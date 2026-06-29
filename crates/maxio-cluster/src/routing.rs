//! Build server-tier routing snapshots from storage Raft HTTP status (production wiring).

use std::time::Duration;

use maxio_common::cluster::{RoutingSnapshot, StorageEndpoint};
use serde::{Deserialize, Serialize};

/// JSON from `GET /internal/raft/status` on a storage node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageRaftStatus {
    pub node_id: u64,
    pub advertise_addr: String,
    pub current_leader: Option<u64>,
    pub is_leader: bool,
    pub quorum_ok: bool,
    pub commit_lag: u64,
}

/// Parsed `id@host:port` or `id=url` peer entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePeerRef {
    pub node_id: String,
    pub status_url: String,
}

/// Parse `1@storage-1:9100,2@storage-2:9100` or `1=http://storage-1:9100/...`.
pub fn parse_storage_peers(raw: &str) -> anyhow::Result<Vec<StoragePeerRef>> {
    let mut out = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (id, rest) = part
            .split_once('@')
            .or_else(|| part.split_once('='))
            .ok_or_else(|| anyhow::anyhow!("invalid storage peer entry: {part}"))?;
        let status_url = if rest.starts_with("http://") || rest.starts_with("https://") {
            let base = rest.trim_end_matches('/');
            format!("{base}/internal/raft/status")
        } else {
            format!("http://{rest}/internal/raft/status")
        };
        out.push(StoragePeerRef {
            node_id: id.to_string(),
            status_url,
        });
    }
    Ok(out)
}

/// Parse `1=http://host:9100,2=http://host2:9100` for Raft HTTP transport.
pub fn parse_raft_peer_urls(raw: &str) -> anyhow::Result<std::collections::BTreeMap<u64, String>> {
    let mut out = std::collections::BTreeMap::new();
    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (id, url) = part
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("invalid raft peer URL entry: {part}"))?;
        let id: u64 = id.parse()?;
        let base = url.trim_end_matches('/');
        out.insert(id, base.to_string());
    }
    Ok(out)
}

pub async fn fetch_storage_status(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<StorageRaftStatus> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(3))
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json().await?)
}

/// Poll all configured storage peers and assemble a routing snapshot for server tier.
pub async fn fetch_routing_snapshot(peers: &[StoragePeerRef]) -> RoutingSnapshot {
    let client = reqwest::Client::new();
    let mut endpoints = Vec::new();
    let mut leader: Option<u64> = None;
    let mut ok_count = 0_u32;

    for peer in peers {
        match fetch_storage_status(&client, &peer.status_url).await {
            Ok(status) => {
                ok_count += 1;
                if status.is_leader {
                    leader = Some(status.node_id);
                } else if leader.is_none() {
                    leader = status.current_leader;
                }
                endpoints.push(StorageEndpoint {
                    node_id: peer.node_id.clone(),
                    addr: status.advertise_addr,
                    is_leader: status.is_leader,
                });
            }
            Err(e) => {
                tracing::warn!(
                    node_id = %peer.node_id,
                    url = %peer.status_url,
                    "storage raft status fetch failed: {e}"
                );
                endpoints.push(StorageEndpoint {
                    node_id: peer.node_id.clone(),
                    addr: String::new(),
                    is_leader: false,
                });
            }
        }
    }

    let quorum_ok = !peers.is_empty() && ok_count as usize * 2 > peers.len() && leader.is_some();

    RoutingSnapshot {
        epoch: 0,
        storage_endpoints: endpoints,
        storage_quorum_ok: quorum_ok,
        credential_epoch: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_storage_peers_at_and_url_forms() {
        let a = parse_storage_peers("1@storage-1:9100,2@storage-2:9100").unwrap();
        assert_eq!(
            a[0].status_url,
            "http://storage-1:9100/internal/raft/status"
        );
        let b = parse_storage_peers("1=http://10.0.0.1:9100").unwrap();
        assert_eq!(b[0].status_url, "http://10.0.0.1:9100/internal/raft/status");
    }

    #[test]
    fn parse_raft_peer_urls() {
        let m = parse_raft_peer_urls("1=http://h1:9100,2=http://h2:9100").unwrap();
        assert_eq!(m.get(&1).unwrap(), "http://h1:9100");
    }
}
