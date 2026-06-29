//! P1-24 multi-node dev harness helpers.

use std::collections::HashMap;
use std::sync::Arc;

use maxio_common::cluster::StorageEndpoint;
use maxio_storage::filesystem::FilesystemStorage;
use maxio_storage::raft::StorageMutation;

use crate::ec::{place_shards, read_shard, write_shard};
use crate::server::ServerCluster;
use crate::storage::StorageCluster;

/// End-to-end 3-node storage + 2-server harness for acceptance tests.
pub struct ClusterHarness {
    pub storage: StorageCluster,
    pub servers: ServerCluster,
}

impl ClusterHarness {
    pub async fn boot() -> anyhow::Result<Self> {
        let storage = StorageCluster::bootstrap_three(true, 4096, 1).await?;
        let servers = ServerCluster::new(2);
        Self::sync_routing(&storage, &servers).await?;
        Ok(Self { storage, servers })
    }

    pub async fn sync_routing(
        storage: &StorageCluster,
        servers: &ServerCluster,
    ) -> anyhow::Result<()> {
        let leader = storage.leader_id().await;
        let endpoints: Vec<StorageEndpoint> = storage
            .nodes()
            .iter()
            .map(|n| StorageEndpoint {
                node_id: n.id.to_string(),
                addr: format!("storage-{}:9100", n.id),
                is_leader: leader == Some(n.id),
            })
            .collect();
        let quorum_ok = storage.nodes().len() >= 3 && leader.is_some();
        servers.publish(endpoints, quorum_ok).await;
        Ok(())
    }

    pub fn fs_map(&self) -> HashMap<String, Arc<FilesystemStorage>> {
        self.storage
            .nodes()
            .iter()
            .map(|n| (n.id.to_string(), n.fs.clone()))
            .collect()
    }

    pub async fn create_bucket(&mut self, name: &str, region: &str) -> anyhow::Result<()> {
        self.storage
            .propose(StorageMutation::CreateBucket {
                name: name.into(),
                region: region.into(),
            })
            .await?;
        Self::sync_routing(&self.storage, &self.servers).await?;
        Ok(())
    }

    pub async fn put_distributed_ec_object(
        &mut self,
        bucket: &str,
        key: &str,
        shards: &[(u32, Vec<u8>)],
    ) -> anyhow::Result<()> {
        let node_ids: Vec<String> = self
            .storage
            .nodes()
            .iter()
            .map(|n| n.id.to_string())
            .collect();
        let data_shards = shards.iter().filter(|(i, _)| *i < 2).count() as u32;
        let map = place_shards(bucket, key, data_shards.max(1), 1, &node_ids);
        self.storage
            .propose(StorageMutation::PutShardMap {
                bucket: bucket.into(),
                key: key.into(),
                map: map.clone(),
            })
            .await?;
        let fs_map = self.fs_map();
        for (idx, bytes) in shards {
            write_shard(&fs_map, &map, *idx, bytes).await?;
        }
        Ok(())
    }

    pub async fn read_shard_after_loss(
        &self,
        bucket: &str,
        key: &str,
        shard: u32,
    ) -> anyhow::Result<Vec<u8>> {
        let leader = self.storage.leader_id().await.unwrap_or(1);
        let placement = self
            .storage
            .node(leader)
            .store
            .shard_map(bucket, key)
            .await
            .ok_or_else(|| anyhow::anyhow!("no shard map"))?;
        read_shard(&self.fs_map(), &placement, shard).await
    }
}
