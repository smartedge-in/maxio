//! Cluster routing and tier identifiers (P1-17 / P1-20).

use serde::{Deserialize, Serialize};

/// Deployable MaxIO tier in a distributed layout (P1-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Ui,
    Server,
    Storage,
}

/// Storage Raft leader endpoint advertised to server tier workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageEndpoint {
    pub node_id: String,
    pub addr: String,
    pub is_leader: bool,
}

/// Snapshot of cluster routing consumed by stateless server pods (P1-20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingSnapshot {
    pub epoch: u64,
    pub storage_endpoints: Vec<StorageEndpoint>,
    #[serde(default)]
    pub storage_quorum_ok: bool,
    #[serde(default)]
    pub credential_epoch: u64,
}

/// Maps EC shard index → owning storage node (P1-18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EcShardPlacement {
    pub bucket: String,
    pub key: String,
    pub data_shards: u32,
    pub parity_shards: u32,
    /// Shard index → storage node id string.
    pub placements: Vec<(u32, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_snapshot_round_trips() {
        let snap = RoutingSnapshot {
            epoch: 3,
            storage_endpoints: vec![StorageEndpoint {
                node_id: "s1".into(),
                addr: "10.0.0.1:9100".into(),
                is_leader: true,
            }],
            storage_quorum_ok: true,
            credential_epoch: 0,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: RoutingSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snap);
    }
}
