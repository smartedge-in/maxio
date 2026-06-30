//! In-memory OpenRaft storage applying [`StorageMutation`] to local filesystem.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::sync::Arc;

use chrono::Utc;
use maxio_storage::BucketMeta;
use maxio_storage::filesystem::FilesystemStorage;
use maxio_storage::raft::StorageMutation;
use openraft::Entry;
use openraft::EntryPayload;
use openraft::LogId;
use openraft::OptionalSend;
use openraft::RaftLogId;
use openraft::RaftLogReader;
use openraft::RaftStorage;
use openraft::RaftTypeConfig;
use openraft::SnapshotMeta;
use openraft::StorageError;
use openraft::StorageIOError;
use openraft::StoredMembership;
use openraft::Vote;
use openraft::storage::LogState;
use openraft::storage::RaftSnapshotBuilder;
use openraft::storage::Snapshot;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::storage::types::{MutationResponse, StorageNodeId, StorageRaftConfig};
use maxio_common::cluster::EcShardPlacement;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct SmData {
    pub last_applied_log: Option<LogId<StorageNodeId>>,
    pub last_membership: StoredMembership<StorageNodeId, openraft::BasicNode>,
    pub buckets: BTreeSet<String>,
    pub shard_maps: HashMap<String, EcShardPlacement>,
}

#[derive(Debug)]
struct StoredSnapshot {
    meta: SnapshotMeta<StorageNodeId, openraft::BasicNode>,
    data: Vec<u8>,
}

pub struct StorageRaftStore {
    fs: Arc<FilesystemStorage>,
    last_purged_log_id: RwLock<Option<LogId<StorageNodeId>>>,
    committed: RwLock<Option<LogId<StorageNodeId>>>,
    log: RwLock<BTreeMap<u64, String>>,
    sm: RwLock<SmData>,
    vote: RwLock<Option<Vote<StorageNodeId>>>,
    snapshot_idx: std::sync::Mutex<u64>,
    current_snapshot: RwLock<Option<StoredSnapshot>>,
}

impl StorageRaftStore {
    pub fn new(fs: Arc<FilesystemStorage>) -> Self {
        Self {
            fs,
            last_purged_log_id: RwLock::new(None),
            committed: RwLock::new(None),
            log: RwLock::new(BTreeMap::new()),
            sm: RwLock::new(SmData::default()),
            vote: RwLock::new(None),
            current_snapshot: RwLock::new(None),
            snapshot_idx: std::sync::Mutex::new(0),
        }
    }

    pub async fn buckets(&self) -> BTreeSet<String> {
        self.sm.read().await.buckets.clone()
    }

    pub async fn shard_map(&self, bucket: &str, key: &str) -> Option<EcShardPlacement> {
        let sm = self.sm.read().await;
        let id = format!("{bucket}/{key}");
        sm.shard_maps.get(&id).cloned()
    }

    pub async fn all_shard_maps(&self) -> Vec<EcShardPlacement> {
        self.sm.read().await.shard_maps.values().cloned().collect()
    }

    async fn apply_mutation(
        &self,
        mutation: &StorageMutation,
    ) -> Result<(), StorageError<StorageNodeId>> {
        match mutation {
            StorageMutation::CreateBucket { name, region } => {
                let meta = BucketMeta {
                    name: name.clone(),
                    created_at: Utc::now().to_rfc3339(),
                    region: region.clone(),
                    versioning: false,
                    cors_rules: None,
                    encryption_config: None,
                    public_read: false,
                    public_list: false,
                    bucket_policy: None,
                    lifecycle_rules: None,
                    erasure_coding: None,
                    tenant_id: None,
                    logging_target_bucket: None,
                    logging_target_prefix: None,
                    notification_config: None,
                    object_lock_enabled: false,
                    object_lock_config: None,
                };
                self.fs
                    .create_bucket(&meta)
                    .await
                    .map_err(|e| StorageIOError::write_state_machine(&e))?;
                self.sm.write().await.buckets.insert(name.clone());
            }
            StorageMutation::DeleteBucket { name } => {
                self.fs
                    .delete_bucket(name)
                    .await
                    .map_err(|e| StorageIOError::write_state_machine(&e))?;
                self.sm.write().await.buckets.remove(name);
            }
            StorageMutation::PutShardMap { bucket, key, map } => {
                let id = format!("{bucket}/{key}");
                self.sm.write().await.shard_maps.insert(id, map.clone());
            }
        }
        Ok(())
    }
}

impl RaftLogReader<StorageRaftConfig> for Arc<StorageRaftStore> {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<StorageRaftConfig>>, StorageError<StorageNodeId>> {
        let mut entries = vec![];
        let log = self.log.read().await;
        for (_, serialized) in log.range(range.clone()) {
            let ent: Entry<StorageRaftConfig> =
                serde_json::from_str(serialized).map_err(|e| StorageIOError::read_logs(&e))?;
            entries.push(ent);
        }
        Ok(entries)
    }
}

impl RaftSnapshotBuilder<StorageRaftConfig> for Arc<StorageRaftStore> {
    async fn build_snapshot(
        &mut self,
    ) -> Result<Snapshot<StorageRaftConfig>, StorageError<StorageNodeId>> {
        let sm = self.sm.read().await;
        let data = serde_json::to_vec(&*sm).map_err(|e| StorageIOError::read_state_machine(&e))?;
        let last_applied_log = sm.last_applied_log;
        let last_membership = sm.last_membership.clone();
        drop(sm);

        let snapshot_idx = {
            let mut l = self.snapshot_idx.lock().unwrap();
            *l += 1;
            *l
        };
        let snapshot_id = if let Some(last) = last_applied_log {
            format!("{}-{}-{}", last.leader_id, last.index, snapshot_idx)
        } else {
            format!("--{snapshot_idx}")
        };

        let meta = SnapshotMeta {
            last_log_id: last_applied_log,
            last_membership,
            snapshot_id,
        };
        let snapshot = StoredSnapshot {
            meta: meta.clone(),
            data: data.clone(),
        };
        *self.current_snapshot.write().await = Some(snapshot);
        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
        })
    }
}

impl RaftStorage<StorageRaftConfig> for Arc<StorageRaftStore> {
    async fn get_log_state(
        &mut self,
    ) -> Result<LogState<StorageRaftConfig>, StorageError<StorageNodeId>> {
        let log = self.log.read().await;
        let last = log
            .iter()
            .next_back()
            .and_then(|(_, s)| serde_json::from_str::<Entry<StorageRaftConfig>>(s).ok())
            .map(|e| *e.get_log_id());
        let last_purged = *self.last_purged_log_id.read().await;
        Ok(LogState {
            last_purged_log_id: last_purged,
            last_log_id: last.or(last_purged),
        })
    }

    async fn save_vote(
        &mut self,
        vote: &Vote<StorageNodeId>,
    ) -> Result<(), StorageError<StorageNodeId>> {
        *self.vote.write().await = Some(*vote);
        Ok(())
    }

    async fn read_vote(
        &mut self,
    ) -> Result<Option<Vote<StorageNodeId>>, StorageError<StorageNodeId>> {
        Ok(*self.vote.read().await)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<StorageNodeId>>,
    ) -> Result<(), StorageError<StorageNodeId>> {
        *self.committed.write().await = committed;
        Ok(())
    }

    async fn read_committed(
        &mut self,
    ) -> Result<Option<LogId<StorageNodeId>>, StorageError<StorageNodeId>> {
        Ok(*self.committed.read().await)
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<
        (
            Option<LogId<StorageNodeId>>,
            StoredMembership<StorageNodeId, openraft::BasicNode>,
        ),
        StorageError<StorageNodeId>,
    > {
        let sm = self.sm.read().await;
        Ok((sm.last_applied_log, sm.last_membership.clone()))
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId<StorageNodeId>,
    ) -> Result<(), StorageError<StorageNodeId>> {
        let mut log = self.log.write().await;
        let keys: Vec<u64> = log.range(log_id.index..).map(|(k, _)| *k).collect();
        for key in keys {
            log.remove(&key);
        }
        Ok(())
    }

    async fn purge_logs_upto(
        &mut self,
        log_id: LogId<StorageNodeId>,
    ) -> Result<(), StorageError<StorageNodeId>> {
        {
            let mut ld = self.last_purged_log_id.write().await;
            *ld = Some(log_id);
        }
        let mut log = self.log.write().await;
        let keys: Vec<u64> = log.range(..=log_id.index).map(|(k, _)| *k).collect();
        for key in keys {
            log.remove(&key);
        }
        Ok(())
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<StorageNodeId>>
    where
        I: IntoIterator<Item = Entry<StorageRaftConfig>> + OptionalSend,
    {
        let mut log = self.log.write().await;
        for entry in entries {
            let s = serde_json::to_string(&entry)
                .map_err(|e| StorageIOError::write_log_entry(*entry.get_log_id(), &e))?;
            log.insert(entry.log_id.index, s);
        }
        Ok(())
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<StorageRaftConfig>],
    ) -> Result<Vec<MutationResponse>, StorageError<StorageNodeId>> {
        let mut res = Vec::with_capacity(entries.len());
        let mut sm = self.sm.write().await;
        for entry in entries {
            sm.last_applied_log = Some(entry.log_id);
            match &entry.payload {
                EntryPayload::Blank => res.push(MutationResponse { ok: true }),
                EntryPayload::Normal(mutation) => {
                    drop(sm);
                    self.apply_mutation(mutation).await?;
                    sm = self.sm.write().await;
                    res.push(MutationResponse { ok: true });
                }
                EntryPayload::Membership(mem) => {
                    sm.last_membership = StoredMembership::new(Some(entry.log_id), mem.clone());
                    res.push(MutationResponse { ok: true });
                }
            }
        }
        Ok(res)
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<<StorageRaftConfig as RaftTypeConfig>::SnapshotData>, StorageError<StorageNodeId>>
    {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<StorageNodeId, openraft::BasicNode>,
        snapshot: Box<<StorageRaftConfig as RaftTypeConfig>::SnapshotData>,
    ) -> Result<(), StorageError<StorageNodeId>> {
        let new_snapshot = StoredSnapshot {
            meta: meta.clone(),
            data: snapshot.into_inner(),
        };
        let new_sm: SmData = serde_json::from_slice(&new_snapshot.data)
            .map_err(|e| StorageIOError::read_snapshot(Some(new_snapshot.meta.signature()), &e))?;
        *self.sm.write().await = new_sm;
        *self.current_snapshot.write().await = Some(new_snapshot);
        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<StorageRaftConfig>>, StorageError<StorageNodeId>> {
        match &*self.current_snapshot.read().await {
            Some(snapshot) => Ok(Some(Snapshot {
                meta: snapshot.meta.clone(),
                snapshot: Box::new(Cursor::new(snapshot.data.clone())),
            })),
            None => Ok(None),
        }
    }

    type LogReader = Self;
    type SnapshotBuilder = Self;

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }
}
