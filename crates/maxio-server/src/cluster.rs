//! Cluster routing snapshot for stateless server replicas (P1-20).

use std::sync::Arc;

use maxio_common::cluster::RoutingSnapshot;
use tokio::sync::RwLock;

/// Shared routing snapshot updated when storage quorum or leader changes.
#[derive(Clone)]
pub struct ClusterState {
    routing: Arc<RwLock<RoutingSnapshot>>,
}

impl Default for ClusterState {
    fn default() -> Self {
        Self::new()
    }
}

impl ClusterState {
    pub fn new() -> Self {
        Self {
            routing: Arc::new(RwLock::new(RoutingSnapshot {
                epoch: 0,
                storage_endpoints: Vec::new(),
                storage_quorum_ok: false,
                credential_epoch: 0,
            })),
        }
    }

    pub fn routing(&self) -> Arc<RwLock<RoutingSnapshot>> {
        self.routing.clone()
    }

    pub async fn publish(&self, snapshot: RoutingSnapshot) {
        *self.routing.write().await = snapshot;
    }

    pub async fn storage_quorum_ok(&self) -> bool {
        self.routing.read().await.storage_quorum_ok
    }

    pub async fn routing_epoch(&self) -> u64 {
        self.routing.read().await.epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxio_common::cluster::StorageEndpoint;

    #[tokio::test]
    async fn publish_updates_quorum_flag() {
        let state = ClusterState::new();
        assert!(!state.storage_quorum_ok().await);
        state
            .publish(RoutingSnapshot {
                epoch: 1,
                storage_endpoints: vec![StorageEndpoint {
                    node_id: "1".into(),
                    addr: "storage-1:9100".into(),
                    is_leader: true,
                }],
                storage_quorum_ok: true,
                credential_epoch: 0,
            })
            .await;
        assert!(state.storage_quorum_ok().await);
        assert_eq!(state.routing_epoch().await, 1);
    }
}
