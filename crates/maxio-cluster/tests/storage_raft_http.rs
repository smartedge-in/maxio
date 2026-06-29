//! Multi-process HTTP Raft smoke test (production wiring).

#![cfg(feature = "http-raft-tests")]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use maxio_cluster::ec::bitrot::BitrotScannerConfig;
use maxio_cluster::routing::fetch_storage_status;
use maxio_cluster::{StorageRaftNode, StorageRaftNodeConfig};
use maxio_storage::raft::StorageMutation;
use tokio::task::JoinHandle;

async fn spawn_node(
    id: u64,
    dir: &tempfile::TempDir,
    port: u16,
    peers: BTreeMap<u64, String>,
    voters: BTreeSet<u64>,
    bootstrap: bool,
) -> JoinHandle<()> {
    let bind = format!("127.0.0.1:{port}");
    let advertise = bind.clone();
    let node = StorageRaftNode::open(StorageRaftNodeConfig {
        node_id: id,
        data_dir: dir.path().to_str().unwrap().to_string(),
        bind_addr: bind.clone(),
        advertise_addr: advertise,
        peer_urls: peers,
        voter_ids: voters,
        bootstrap,
        erasure_coding: false,
        chunk_size: 1024 * 1024,
        parity_shards: 0,
        metadata_index: false,
        bitrot_scan_enabled: false,
        bitrot_scan_interval_secs: 3600,
    })
    .await
    .unwrap();

    let bitrot = BitrotScannerConfig {
        local_node_id: id.to_string(),
        interval: Duration::from_secs(3600),
        enabled: false,
    };
    tokio::spawn(async move {
        node.serve(&bind, bitrot).await.unwrap();
    })
}

fn peer_map(ports: &[u16]) -> BTreeMap<u64, String> {
    ports
        .iter()
        .enumerate()
        .map(|(i, p)| ((i as u64) + 1, format!("http://127.0.0.1:{p}")))
        .collect()
}

#[tokio::test]
async fn http_raft_three_node_bootstrap_and_propose() {
    let ports = [19101_u16, 19102, 19103];
    let peers = peer_map(&ports);
    let voters: BTreeSet<u64> = [1_u64, 2, 3].into();
    let client = reqwest::Client::new();

    let mut handles = Vec::new();
    for (i, port) in ports.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let id = (i as u64) + 1;
        let h = spawn_node(id, &dir, *port, peers.clone(), voters.clone(), id == 1).await;
        handles.push((dir, h));
    }

    tokio::time::sleep(Duration::from_secs(3)).await;

    let status = fetch_storage_status(&client, "http://127.0.0.1:19101/internal/raft/status")
        .await
        .unwrap();
    assert!(status.quorum_ok || status.current_leader.is_some());

    let leader_port = if status.is_leader {
        ports[0]
    } else {
        ports[status.current_leader.unwrap_or(1) as usize - 1]
    };
    let propose_url = format!("http://127.0.0.1:{leader_port}/internal/raft/propose");

    let resp = client
        .post(&propose_url)
        .json(&StorageMutation::CreateBucket {
            name: "http-bucket".into(),
            region: "us-east-1".into(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<maxio_cluster::storage::MutationResponse>()
        .await
        .unwrap();
    assert!(resp.ok);

    tokio::time::sleep(Duration::from_millis(500)).await;
    for port in ports {
        let st = fetch_storage_status(
            &client,
            &format!("http://127.0.0.1:{port}/internal/raft/status"),
        )
        .await
        .unwrap();
        assert!(st.current_leader.is_some());
    }
}
