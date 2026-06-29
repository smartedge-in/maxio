//! In-process Raft RPC router for cluster tests (P1-24).

use std::collections::BTreeMap;
use std::sync::Arc;

use openraft::BasicNode;
use openraft::error::Fatal;
use openraft::error::InstallSnapshotError;
use openraft::error::RPCError;
use openraft::error::RaftError;
use openraft::error::RemoteError;
use openraft::network::RPCOption;
use openraft::network::RaftNetwork;
use openraft::network::RaftNetworkFactory;
use openraft::raft::AppendEntriesRequest;
use openraft::raft::AppendEntriesResponse;
use openraft::raft::InstallSnapshotRequest;
use openraft::raft::InstallSnapshotResponse;
use openraft::raft::VoteRequest;
use openraft::raft::VoteResponse;
use tokio::sync::RwLock;

use crate::storage::types::{StorageNodeId, StorageRaft, StorageRaftConfig};

#[derive(Clone)]
pub struct ClusterRouter {
    pub handles: Arc<RwLock<BTreeMap<StorageNodeId, RaftHandle>>>,
}

impl Default for ClusterRouter {
    fn default() -> Self {
        Self {
            handles: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
}

#[derive(Clone)]
pub struct RaftHandle {
    pub raft: StorageRaft,
}

impl ClusterRouter {
    pub async fn register(&self, id: StorageNodeId, handle: RaftHandle) {
        self.handles.write().await.insert(id, handle);
    }
}

impl RaftNetworkFactory<StorageRaftConfig> for ClusterRouter {
    type Network = DirectNetwork;

    async fn new_client(&mut self, target: StorageNodeId, _node: &BasicNode) -> Self::Network {
        DirectNetwork {
            handles: self.handles.clone(),
            target,
        }
    }
}

pub struct DirectNetwork {
    handles: Arc<RwLock<BTreeMap<StorageNodeId, RaftHandle>>>,
    target: StorageNodeId,
}

impl DirectNetwork {
    async fn raft(&self) -> Option<StorageRaft> {
        self.handles
            .read()
            .await
            .get(&self.target)
            .map(|h| h.raft.clone())
    }
}

impl RaftNetwork<StorageRaftConfig> for DirectNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<StorageRaftConfig>,
        _option: RPCOption,
    ) -> Result<
        AppendEntriesResponse<StorageNodeId>,
        RPCError<StorageNodeId, BasicNode, RaftError<StorageNodeId>>,
    > {
        let raft = self.raft().await.ok_or_else(|| {
            RPCError::RemoteError(RemoteError::new(
                self.target,
                RaftError::Fatal(Fatal::Stopped),
            ))
        })?;
        raft.append_entries(rpc)
            .await
            .map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<StorageRaftConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<StorageNodeId>,
        RPCError<StorageNodeId, BasicNode, RaftError<StorageNodeId, InstallSnapshotError>>,
    > {
        let raft = self.raft().await.ok_or_else(|| {
            RPCError::RemoteError(RemoteError::new(
                self.target,
                RaftError::Fatal(Fatal::Stopped),
            ))
        })?;
        raft.install_snapshot(rpc)
            .await
            .map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<StorageNodeId>,
        _option: RPCOption,
    ) -> Result<
        VoteResponse<StorageNodeId>,
        RPCError<StorageNodeId, BasicNode, RaftError<StorageNodeId>>,
    > {
        let raft = self.raft().await.ok_or_else(|| {
            RPCError::RemoteError(RemoteError::new(
                self.target,
                RaftError::Fatal(Fatal::Stopped),
            ))
        })?;
        raft.vote(rpc)
            .await
            .map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }
}
