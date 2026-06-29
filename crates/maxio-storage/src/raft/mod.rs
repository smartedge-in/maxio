//! Storage tier Raft consensus (P1-17).
//!
//! Metadata mutations (buckets, object index, multipart state, keyring epoch) will be
//! ordered through a Raft log and applied to the local [`StorageBackend`](crate::backend::StorageBackend).
//! Object bytes remain on local disk per storage node.

use maxio_common::cluster::StorageEndpoint;
use serde::{Deserialize, Serialize};

/// Bootstrap configuration for one storage Raft peer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaftNodeConfig {
    pub node_id: String,
    pub bind_addr: String,
    pub peers: Vec<StorageEndpoint>,
}

/// Ordered mutation applied on the Raft leader and followers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StorageMutation {
    CreateBucket {
        name: String,
        region: String,
    },
    DeleteBucket {
        name: String,
    },
    /// Distributed EC shard placement map (P1-18) — replicated metadata only.
    PutShardMap {
        bucket: String,
        key: String,
        map: maxio_common::cluster::EcShardPlacement,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raft_node_config_serializes() {
        let cfg = RaftNodeConfig {
            node_id: "storage-1".into(),
            bind_addr: "127.0.0.1:9100".into(),
            peers: vec![StorageEndpoint {
                node_id: "storage-2".into(),
                addr: "127.0.0.1:9101".into(),
                is_leader: false,
            }],
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: RaftNodeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn storage_mutation_tagged_json() {
        let m = StorageMutation::CreateBucket {
            name: "logs".into(),
            region: "us-east-1".into(),
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"op\":\"create_bucket\""));
    }
}
