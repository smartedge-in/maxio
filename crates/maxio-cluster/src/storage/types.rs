//! OpenRaft type configuration for storage tier.

use std::io::Cursor;

use maxio_storage::raft::StorageMutation;
use openraft::BasicNode;
use serde::{Deserialize, Serialize};

pub type StorageNodeId = u64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationResponse {
    pub ok: bool,
}

openraft::declare_raft_types!(
    pub StorageRaftConfig:
        D = StorageMutation,
        R = MutationResponse,
        NodeId = StorageNodeId,
        Node = BasicNode,
        Entry = openraft::Entry<StorageRaftConfig>,
        SnapshotData = Cursor<Vec<u8>>,
        AsyncRuntime = openraft::TokioRuntime,
);

pub type StorageRaft = openraft::Raft<StorageRaftConfig>;
