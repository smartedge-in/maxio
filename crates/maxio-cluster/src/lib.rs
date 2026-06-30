//! Multi-replica cluster coordination (P1-14 epic).
//!
//! - **Storage Raft** (P1-17): metadata consensus via OpenRaft
//! - **Distributed EC** (P1-18/P1-19): shard placement + peer fetch
//! - **Server routing** (P1-20): replicated `RoutingSnapshot`
//! - **Harness** (P1-24): in-process 3-node tests

pub mod client;
pub mod ec;
pub use ec::cluster_shard_path;
pub mod harness;
pub mod metadata;
pub mod routing;
pub mod server;
pub mod storage;

pub use client::StorageRaftClient;
pub use ec::EcShardMap;
pub use ec::bitrot::{BitrotMetrics, BitrotScannerConfig};
pub use harness::ClusterHarness;
pub use metadata::{ClusterMetadataStorage, wrap_cluster_storage};
pub use routing::{
    StoragePeerRef, StorageRaftStatus, fetch_routing_snapshot, parse_raft_peer_urls,
    parse_storage_peers,
};
pub use server::ServerCluster;
pub use storage::{StorageCluster, StorageRaftNode, StorageRaftNodeConfig};
