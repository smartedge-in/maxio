//! Storage tier Raft (P1-17).

pub mod api;
pub mod http_network;
pub mod metrics;
pub mod network;
pub mod node;
pub mod raft_store;
pub mod types;

pub use metrics::StorageRaftMetrics;
pub use network::ClusterRouter;
pub use node::{StorageRaftNode, StorageRaftNodeConfig, StorageRaftNodeHandle};
pub use raft_store::StorageRaftStore;
pub use types::{MutationResponse, StorageRaft, StorageRaftConfig};

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;
use std::time::Duration;

use maxio_storage::filesystem::FilesystemStorage;
use maxio_storage::keys::Keyring;
use maxio_storage::quota::QuotaLimits;
use openraft::Config;
use openraft::LogIdOptionExt;
use openraft::Raft;
use tempfile::TempDir;
use tokio::sync::RwLock;

use crate::storage::network::RaftHandle;
use crate::storage::types::StorageNodeId;

/// A single storage Raft peer.
pub struct StorageNode {
    pub id: StorageNodeId,
    pub data_dir: TempDir,
    pub raft: StorageRaft,
    pub store: Arc<StorageRaftStore>,
    pub fs: Arc<FilesystemStorage>,
}

/// Three-node (or N-node) storage quorum.
pub struct StorageCluster {
    router: ClusterRouter,
    nodes: Vec<StorageNode>,
    dead: Arc<RwLock<HashSet<StorageNodeId>>>,
}

impl StorageCluster {
    /// Bootstrap a 3-node in-process storage cluster for tests and harness scripts.
    pub async fn bootstrap_three(
        erasure_coding: bool,
        chunk_size: u64,
        parity_shards: u32,
    ) -> anyhow::Result<Self> {
        Self::bootstrap_nodes(&[1_u64, 2, 3], erasure_coding, chunk_size, parity_shards).await
    }

    pub async fn bootstrap_nodes(
        ids: &[StorageNodeId],
        erasure_coding: bool,
        chunk_size: u64,
        parity_shards: u32,
    ) -> anyhow::Result<Self> {
        let config = Arc::new(
            Config {
                heartbeat_interval: 200,
                election_timeout_min: 800,
                election_timeout_max: 1200,
                ..Default::default()
            }
            .validate()?,
        );

        let router = ClusterRouter::default();
        let mut nodes = Vec::new();

        for &id in ids {
            let dir = tempfile::tempdir()?;
            let data_path = dir.path().to_str().unwrap().to_string();
            let keyring = Arc::new(Keyring::load(&data_path, None).await?);
            let fs = Arc::new(
                FilesystemStorage::new(
                    &data_path,
                    erasure_coding,
                    chunk_size,
                    parity_shards,
                    keyring,
                    None,
                    QuotaLimits::from_config(0, 0),
                    false,
                )
                .await?,
            );
            let store = Arc::new(StorageRaftStore::new(fs.clone()));
            let (log_store, sm_store) = openraft::storage::Adaptor::new(store.clone());

            let raft = Raft::new(id, config.clone(), router.clone(), log_store, sm_store).await?;

            router.register(id, RaftHandle { raft: raft.clone() }).await;

            nodes.push(StorageNode {
                id,
                data_dir: dir,
                raft,
                store,
                fs,
            });
        }

        let voters: BTreeSet<StorageNodeId> = ids.iter().copied().collect();
        let init = nodes[0].raft.clone();
        init.initialize(voters).await?;

        // Wait for leader + membership log applied on all nodes.
        tokio::time::sleep(Duration::from_millis(500)).await;
        Self::wait_applied_index(&nodes, 1).await?;

        Ok(Self {
            router,
            nodes,
            dead: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    pub fn router(&self) -> &ClusterRouter {
        &self.router
    }

    pub fn nodes(&self) -> &[StorageNode] {
        &self.nodes
    }

    pub fn node(&self, id: StorageNodeId) -> &StorageNode {
        self.nodes.iter().find(|n| n.id == id).expect("node id")
    }

    pub async fn leader_id(&self) -> Option<StorageNodeId> {
        let dead = self.dead.read().await;
        for node in &self.nodes {
            if dead.contains(&node.id) {
                continue;
            }
            if node.raft.current_leader().await == Some(node.id) {
                return Some(node.id);
            }
        }
        for node in &self.nodes {
            if dead.contains(&node.id) {
                continue;
            }
            if let Some(leader) = node.raft.current_leader().await
                && !dead.contains(&leader)
            {
                return Some(leader);
            }
        }
        None
    }

    pub async fn propose(
        &self,
        mutation: maxio_storage::raft::StorageMutation,
    ) -> anyhow::Result<MutationResponse> {
        let leader_id = self
            .leader_id()
            .await
            .ok_or_else(|| anyhow::anyhow!("no storage raft leader"))?;
        let leader = self.node(leader_id);
        let resp = leader.raft.client_write(mutation).await?;
        self.wait_applied_on_alive(resp.log_id.index).await?;
        Ok(resp.data)
    }

    /// Simulate leader loss by shutting down the current leader's Raft task.
    pub async fn kill_leader(&self) -> anyhow::Result<Option<StorageNodeId>> {
        let old = self.leader_id().await;
        if let Some(id) = old {
            let node = self.node(id);
            node.raft.shutdown().await?;
            router_unregister(&self.router, id).await;
            self.dead.write().await.insert(id);
        }
        tokio::time::sleep(Duration::from_millis(1200)).await;
        Ok(old)
    }

    pub async fn wait_leader(&self) -> anyhow::Result<StorageNodeId> {
        for _ in 0..60 {
            if let Some(id) = self.leader_id().await
                && !self.dead.read().await.contains(&id)
            {
                return Ok(id);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        anyhow::bail!("timed out waiting for storage leader")
    }

    pub async fn metrics(&self) -> StorageRaftMetrics {
        let leader = self.leader_id().await;
        let mut commit_lag = 0_u64;
        if let Some(id) = leader {
            let m = self.node(id).raft.metrics().borrow().clone();
            if let (Some(last), Some(applied)) = (m.last_log_index, m.last_applied.index()) {
                commit_lag = last.saturating_sub(applied);
            }
        }
        StorageRaftMetrics {
            leader_node: leader,
            commit_lag,
        }
    }

    async fn wait_applied_index(nodes: &[StorageNode], index: u64) -> anyhow::Result<()> {
        for node in nodes {
            for _ in 0..50 {
                let m = node.raft.metrics().borrow().clone();
                if m.last_applied.index() >= Some(index) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        Ok(())
    }

    async fn wait_applied_on_alive(&self, index: u64) -> anyhow::Result<()> {
        let dead = self.dead.read().await;
        for node in &self.nodes {
            if dead.contains(&node.id) {
                continue;
            }
            for _ in 0..50 {
                let m = node.raft.metrics().borrow().clone();
                if m.last_applied.index() >= Some(index) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        Ok(())
    }
}

async fn router_unregister(router: &ClusterRouter, id: StorageNodeId) {
    let mut guard = router.handles.write().await;
    guard.remove(&id);
}
