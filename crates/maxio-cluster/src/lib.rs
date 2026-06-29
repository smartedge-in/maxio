//! Multi-replica cluster coordination (P1-14 epic).
//!
//! - **Storage Raft** (P1-17): metadata consensus via OpenRaft
//! - **Distributed EC** (P1-18/P1-19): shard placement + peer fetch
//! - **Server routing** (P1-20): replicated `RoutingSnapshot`
//! - **Harness** (P1-24): in-process 3-node tests

pub mod ec;
pub mod harness;
pub mod server;
pub mod storage;

pub use ec::EcShardMap;
pub use harness::ClusterHarness;
pub use server::ServerCluster;
pub use storage::StorageCluster;
