//! HTTP Raft transport for multi-process storage clusters.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use openraft::BasicNode;
use openraft::error::InstallSnapshotError;
use openraft::error::NetworkError;
use openraft::error::RPCError;
use openraft::error::RaftError;
use openraft::network::RPCOption;
use openraft::network::RaftNetwork;
use openraft::network::RaftNetworkFactory;
use openraft::raft::AppendEntriesRequest;
use openraft::raft::AppendEntriesResponse;
use openraft::raft::InstallSnapshotRequest;
use openraft::raft::InstallSnapshotResponse;
use openraft::raft::VoteRequest;
use openraft::raft::VoteResponse;

use crate::storage::types::{StorageNodeId, StorageRaftConfig};

#[derive(Clone)]
pub struct HttpRaftNetworkFactory {
    peers: Arc<BTreeMap<StorageNodeId, String>>,
    client: reqwest::Client,
}

impl HttpRaftNetworkFactory {
    pub fn new(peers: BTreeMap<StorageNodeId, String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            peers: Arc::new(peers),
            client,
        }
    }
}

pub struct HttpRaftNetwork {
    base_url: String,
    client: reqwest::Client,
}

impl HttpRaftNetwork {
    async fn post_rpc<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        req: Req,
    ) -> Result<Resp, RPCError<StorageNodeId, BasicNode, RaftError<StorageNodeId>>> {
        let url = format!(
            "{}/internal/raft/{path}",
            self.base_url.trim_end_matches('/')
        );
        let resp = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&e)))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let io_err =
                std::io::Error::other(format!("HTTP {status}: {body}"));
            return Err(RPCError::Network(NetworkError::new(&io_err)));
        }
        resp.json()
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&e)))
    }
}

impl RaftNetworkFactory<StorageRaftConfig> for HttpRaftNetworkFactory {
    type Network = HttpRaftNetwork;

    async fn new_client(&mut self, target: StorageNodeId, _node: &BasicNode) -> Self::Network {
        let base_url = self
            .peers
            .get(&target)
            .cloned()
            .unwrap_or_else(|| format!("http://127.0.0.1:{target}"));
        HttpRaftNetwork {
            base_url,
            client: self.client.clone(),
        }
    }
}

impl RaftNetwork<StorageRaftConfig> for HttpRaftNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<StorageRaftConfig>,
        _option: RPCOption,
    ) -> Result<
        AppendEntriesResponse<StorageNodeId>,
        RPCError<StorageNodeId, BasicNode, RaftError<StorageNodeId>>,
    > {
        self.post_rpc("append_entries", rpc).await
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<StorageRaftConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<StorageNodeId>,
        RPCError<StorageNodeId, BasicNode, RaftError<StorageNodeId, InstallSnapshotError>>,
    > {
        let url = format!(
            "{}/internal/raft/install_snapshot",
            self.base_url.trim_end_matches('/')
        );
        let resp = self
            .client
            .post(&url)
            .json(&rpc)
            .send()
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&e)))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let io_err =
                std::io::Error::other(format!("HTTP {status}: {body}"));
            return Err(RPCError::Network(NetworkError::new(&io_err)));
        }
        resp.json()
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&e)))
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<StorageNodeId>,
        _option: RPCOption,
    ) -> Result<
        VoteResponse<StorageNodeId>,
        RPCError<StorageNodeId, BasicNode, RaftError<StorageNodeId>>,
    > {
        self.post_rpc("vote", rpc).await
    }
}
