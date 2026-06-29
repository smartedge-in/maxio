//! Server tier routing snapshot (P1-20).

use std::sync::Arc;

use maxio_common::cluster::{RoutingSnapshot, StorageEndpoint};
use tokio::sync::RwLock;

/// Stateless server replica holding a cached routing snapshot.
#[derive(Clone)]
pub struct ServerReplica {
    pub id: String,
    pub routing: Arc<RwLock<RoutingSnapshot>>,
}

/// Two-or-more server pods sharing a replicated routing snapshot.
pub struct ServerCluster {
    pub routing: Arc<RwLock<RoutingSnapshot>>,
    pub replicas: Vec<ServerReplica>,
}

impl ServerCluster {
    pub fn new(replica_count: usize) -> Self {
        let routing = Arc::new(RwLock::new(RoutingSnapshot {
            epoch: 0,
            storage_endpoints: Vec::new(),
            storage_quorum_ok: false,
            credential_epoch: 0,
        }));
        let replicas: Vec<ServerReplica> = (0..replica_count)
            .map(|i| ServerReplica {
                id: format!("server-{i}"),
                routing: routing.clone(),
            })
            .collect();
        Self { routing, replicas }
    }

    /// Publish a new routing snapshot (simulates Server Raft commit).
    pub async fn publish(&self, endpoints: Vec<StorageEndpoint>, quorum_ok: bool) {
        let mut snap = self.routing.write().await;
        snap.epoch = snap.epoch.saturating_add(1);
        snap.storage_endpoints = endpoints;
        snap.storage_quorum_ok = quorum_ok;
    }

    pub async fn snapshot(&self) -> RoutingSnapshot {
        self.routing.read().await.clone()
    }

    pub async fn readyz_ok(&self) -> bool {
        self.routing.read().await.storage_quorum_ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn two_replicas_share_routing_epoch() {
        let cluster = ServerCluster::new(2);
        cluster
            .publish(
                vec![StorageEndpoint {
                    node_id: "s1".into(),
                    addr: "10.0.0.1:9100".into(),
                    is_leader: true,
                }],
                true,
            )
            .await;
        assert_eq!(cluster.replicas[0].routing.read().await.epoch, 1);
        assert_eq!(cluster.replicas[1].routing.read().await.epoch, 1);
        assert!(cluster.readyz_ok().await);
    }
}
