//! HTTP client for storage-tier Raft (leader discovery + mutation propose).

use std::time::Duration;

use maxio_storage::raft::StorageMutation;

use crate::routing::{StoragePeerRef, StorageRaftStatus, fetch_storage_status};
use crate::storage::types::MutationResponse;

/// Client that discovers the storage Raft leader and submits [`StorageMutation`]s.
#[derive(Clone)]
pub struct StorageRaftClient {
    client: reqwest::Client,
    peers: Vec<StoragePeerRef>,
}

impl StorageRaftClient {
    pub fn new(peers: Vec<StoragePeerRef>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client, peers }
    }

    /// Base URL for `POST /internal/raft/propose` derived from a status URL.
    pub fn propose_url_from_status(status_url: &str) -> String {
        status_url.replace("/internal/raft/status", "/internal/raft/propose")
    }

    /// Discover the current leader's propose endpoint by polling configured peers.
    pub async fn find_leader_propose_url(&self) -> anyhow::Result<String> {
        let mut leader_id: Option<u64> = None;
        let mut by_node_id: Vec<(StoragePeerRef, StorageRaftStatus)> = Vec::new();

        for peer in &self.peers {
            match fetch_storage_status(&self.client, &peer.status_url).await {
                Ok(status) => {
                    if status.is_leader {
                        return Ok(Self::propose_url_from_status(&peer.status_url));
                    }
                    if leader_id.is_none() {
                        leader_id = status.current_leader;
                    }
                    by_node_id.push((peer.clone(), status));
                }
                Err(e) => {
                    tracing::warn!(
                        node_id = %peer.node_id,
                        url = %peer.status_url,
                        "storage raft status fetch failed: {e}"
                    );
                }
            }
        }

        if let Some(lid) = leader_id {
            for (peer, status) in &by_node_id {
                if status.node_id == lid {
                    return Ok(Self::propose_url_from_status(&peer.status_url));
                }
            }
            for peer in &self.peers {
                if peer.node_id == lid.to_string() {
                    return Ok(Self::propose_url_from_status(&peer.status_url));
                }
            }
        }

        anyhow::bail!(
            "no storage raft leader found among {} peer(s)",
            self.peers.len()
        )
    }

    /// Submit a metadata mutation to the current storage Raft leader.
    pub async fn propose(&self, mutation: StorageMutation) -> anyhow::Result<MutationResponse> {
        let url = self.find_leader_propose_url().await?;
        let resp = self
            .client
            .post(&url)
            .json(&mutation)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn propose_url_from_status_replaces_path() {
        assert_eq!(
            StorageRaftClient::propose_url_from_status(
                "http://storage-1:9100/internal/raft/status"
            ),
            "http://storage-1:9100/internal/raft/propose"
        );
    }
}
