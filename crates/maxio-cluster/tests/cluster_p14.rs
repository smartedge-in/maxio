//! P1-14 epic acceptance tests (P1-17–P1-21, P1-24).

#![cfg(feature = "cluster-tests")]

use std::io::Cursor;

use maxio_cluster::StorageCluster;
use maxio_cluster::ec::{place_shards, write_shard, write_shard_on_node};
use maxio_cluster::harness::ClusterHarness;
use maxio_storage::ByteStream;
use maxio_storage::raft::StorageMutation;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn storage_raft_three_node_create_bucket() {
    let cluster = StorageCluster::bootstrap_three(false, 1024 * 1024, 0)
        .await
        .unwrap();
    cluster
        .propose(StorageMutation::CreateBucket {
            name: "logs".into(),
            region: "us-east-1".into(),
        })
        .await
        .unwrap();

    for node in cluster.nodes() {
        assert!(node.fs.head_bucket("logs").await.unwrap());
    }
}

#[tokio::test]
async fn storage_raft_leader_failover() {
    let cluster = StorageCluster::bootstrap_three(false, 1024 * 1024, 0)
        .await
        .unwrap();
    let old_leader = cluster.leader_id().await.unwrap();
    cluster.kill_leader().await.unwrap();
    let new_leader = cluster.wait_leader().await.unwrap();
    assert_ne!(old_leader, new_leader);

    cluster
        .propose(StorageMutation::CreateBucket {
            name: "after-failover".into(),
            region: "us-east-1".into(),
        })
        .await
        .unwrap();

    for node in cluster.nodes() {
        if node.id != old_leader {
            assert!(node.fs.head_bucket("after-failover").await.unwrap());
        }
    }
}

#[tokio::test]
async fn storage_raft_metrics_export() {
    let cluster = StorageCluster::bootstrap_three(false, 1024 * 1024, 0)
        .await
        .unwrap();
    let m = cluster.metrics().await;
    let prom = m.render_prometheus();
    assert!(prom.contains("raft_storage_leader"));
    assert!(prom.contains("raft_storage_commit_lag"));
}

#[tokio::test]
async fn storage_raft_put_get_object_on_leader() {
    let cluster = StorageCluster::bootstrap_three(false, 1024 * 1024, 0)
        .await
        .unwrap();
    cluster
        .propose(StorageMutation::CreateBucket {
            name: "objects".into(),
            region: "us-east-1".into(),
        })
        .await
        .unwrap();

    let leader_id = cluster.leader_id().await.unwrap();
    let leader = cluster.node(leader_id);
    let body: ByteStream = Box::pin(Cursor::new(b"hello-raft".to_vec()));
    leader
        .fs
        .put_object(
            "objects",
            "greeting.txt",
            "text/plain",
            body,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    let (mut stream, meta) = leader
        .fs
        .get_object("objects", "greeting.txt", None)
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, b"hello-raft");
    assert_eq!(meta.size, 10);
}

#[tokio::test]
async fn distributed_ec_shard_placement_and_peer_read() {
    let mut h = ClusterHarness::boot().await.unwrap();
    h.create_bucket("ec-bucket", "us-east-1").await.unwrap();
    h.put_distributed_ec_object(
        "ec-bucket",
        "obj.ec",
        &[
            (0, b"shard0-data".to_vec()),
            (1, b"shard1-data".to_vec()),
            (2, b"parity0".to_vec()),
        ],
    )
    .await
    .unwrap();

    let bytes = h
        .read_shard_after_loss("ec-bucket", "obj.ec", 0)
        .await
        .unwrap();
    assert_eq!(bytes, b"shard0-data");
}

#[tokio::test]
async fn distributed_ec_induced_shard_loss_peer_fetch() {
    let mut h = ClusterHarness::boot().await.unwrap();
    h.create_bucket("loss-bucket", "us-east-1").await.unwrap();

    let node_ids: Vec<String> = h.storage.nodes().iter().map(|n| n.id.to_string()).collect();
    let map = place_shards("loss-bucket", "obj.ec", 2, 1, &node_ids);
    h.storage
        .propose(StorageMutation::PutShardMap {
            bucket: "loss-bucket".into(),
            key: "obj.ec".into(),
            map: map.clone(),
        })
        .await
        .unwrap();

    let fs_map = h.fs_map();
    write_shard(&fs_map, &map, 1, b"shard1-data").await.unwrap();
    write_shard(&fs_map, &map, 2, b"parity0").await.unwrap();

    let owner = map
        .placements
        .iter()
        .find(|(i, _)| *i == 0)
        .map(|(_, n)| n.clone())
        .unwrap();
    let peer = node_ids.iter().find(|id| *id != &owner).unwrap().clone();

    // Shard 0 is mapped to `owner` but only exists on `peer` (owner loss).
    write_shard_on_node(
        &fs_map,
        &peer,
        "loss-bucket",
        "obj.ec",
        0,
        b"shard0-via-peer",
    )
    .await
    .unwrap();

    let bytes = h
        .read_shard_after_loss("loss-bucket", "obj.ec", 0)
        .await
        .unwrap();
    assert_eq!(bytes, b"shard0-via-peer");
}

#[tokio::test]
async fn reconstruct_shard_after_storage_node_down() {
    use maxio_cluster::ec::{cluster_shard_path, place_shards, reconstruct_shard, write_shard};
    use maxio_storage::raft::StorageMutation;

    let mut h = ClusterHarness::boot().await.unwrap();
    h.create_bucket("down-bucket", "us-east-1").await.unwrap();

    let node_ids: Vec<String> = h.storage.nodes().iter().map(|n| n.id.to_string()).collect();
    let map = place_shards("down-bucket", "obj.ec", 2, 1, &node_ids);
    h.storage
        .propose(StorageMutation::PutShardMap {
            bucket: "down-bucket".into(),
            key: "obj.ec".into(),
            map: map.clone(),
        })
        .await
        .unwrap();

    let s0 = b"shard0-data".to_vec();
    let s1 = b"shard1-data".to_vec();
    let shard_size = s0.len();
    let mut parity = vec![0u8; shard_size];
    {
        use reed_solomon_erasure::galois_8::ReedSolomon;
        let rs = ReedSolomon::new(2, 1).unwrap();
        let mut all = vec![s0.clone(), s1.clone(), parity.clone()];
        rs.encode(&mut all).unwrap();
        parity = all[2].clone();
    }

    let fs_map = h.fs_map();
    write_shard(&fs_map, &map, 0, &s0).await.unwrap();
    write_shard(&fs_map, &map, 1, &s1).await.unwrap();
    write_shard(&fs_map, &map, 2, &parity).await.unwrap();

    let down_node = map
        .placements
        .iter()
        .find(|(i, _)| *i == 0)
        .map(|(_, n)| n.clone())
        .unwrap();
    let mut reduced_map = fs_map;
    reduced_map.remove(&down_node);

    let mut present = Vec::new();
    for idx in [1u32, 2] {
        for fs in reduced_map.values() {
            let path = cluster_shard_path(fs.data_root(), "down-bucket", "obj.ec", idx);
            if let Ok(bytes) = tokio::fs::read(&path).await {
                present.push((idx, bytes));
                break;
            }
        }
    }
    assert_eq!(present.len(), 2, "need surviving data shard + parity");

    let rebuilt = reconstruct_shard(2, 1, shard_size, &present, 0).unwrap();
    assert_eq!(rebuilt, s0);
}

#[tokio::test]
async fn bitrot_scanner_heals_corrupt_local_shard() {
    use maxio_cluster::ec::bitrot::{BitrotMetrics, scan_placements, verify_shard_checksum};
    use maxio_cluster::ec::{cluster_shard_path, place_shards, write_shard};

    let mut h = ClusterHarness::boot().await.unwrap();
    h.create_bucket("bitrot-bucket", "us-east-1").await.unwrap();

    let node_ids: Vec<String> = h.storage.nodes().iter().map(|n| n.id.to_string()).collect();
    let map = place_shards("bitrot-bucket", "obj.ec", 2, 1, &node_ids);
    h.storage
        .propose(StorageMutation::PutShardMap {
            bucket: "bitrot-bucket".into(),
            key: "obj.ec".into(),
            map: map.clone(),
        })
        .await
        .unwrap();

    let fs_map = h.fs_map();
    let s0 = b"shard0-data".to_vec();
    let s1 = b"shard1-data".to_vec();
    let mut parity = vec![0u8; s0.len()];
    {
        use reed_solomon_erasure::galois_8::ReedSolomon;
        let rs = ReedSolomon::new(2, 1).unwrap();
        let mut all = vec![s0.clone(), s1.clone(), parity.clone()];
        rs.encode(&mut all).unwrap();
        parity = all[2].clone();
    }
    write_shard(&fs_map, &map, 0, &s0).await.unwrap();
    write_shard(&fs_map, &map, 1, &s1).await.unwrap();
    write_shard(&fs_map, &map, 2, &parity).await.unwrap();

    let owner = map
        .placements
        .iter()
        .find(|(i, _)| *i == 0)
        .map(|(_, n)| n.clone())
        .unwrap();
    let owner_fs = fs_map.get(&owner).unwrap();
    let path = cluster_shard_path(owner_fs.data_root(), "bitrot-bucket", "obj.ec", 0);
    tokio::fs::write(&path, b"CORRUPT!!!").await.unwrap();

    let metrics = BitrotMetrics::default();
    scan_placements(
        owner_fs,
        &owner,
        std::slice::from_ref(&map),
        &std::collections::BTreeMap::new(),
        Some(&fs_map),
        &metrics,
    )
    .await;

    assert_eq!(
        metrics
            .corrupt_detected
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(
        metrics
            .shards_healed
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    let healed = tokio::fs::read(&path).await.unwrap();
    assert_eq!(healed, s0);
    assert!(verify_shard_checksum(&path, &healed).await.unwrap());
}

#[tokio::test]
async fn server_routing_survives_storage_leader_change() {
    let h = ClusterHarness::boot().await.unwrap();
    let epoch_before = h.servers.snapshot().await.epoch;
    let old = h.storage.leader_id().await.unwrap();
    h.storage.kill_leader().await.unwrap();
    h.storage.wait_leader().await.unwrap();
    ClusterHarness::sync_routing(&h.storage, &h.servers)
        .await
        .unwrap();
    let snap = h.servers.snapshot().await;
    assert!(snap.epoch > epoch_before);
    assert!(snap.storage_quorum_ok);
    assert!(snap.storage_endpoints.iter().any(|e| e.is_leader));
    assert!(h.servers.readyz_ok().await);
    assert_ne!(old, h.storage.leader_id().await.unwrap());
}

#[tokio::test]
async fn harness_put_get_smoke() {
    let mut h = ClusterHarness::boot().await.unwrap();
    h.create_bucket("smoke", "us-east-1").await.unwrap();
    let leader = h.storage.leader_id().await.unwrap();
    assert!(
        h.storage
            .node(leader)
            .fs
            .head_bucket("smoke")
            .await
            .unwrap()
    );
    assert_eq!(h.servers.replicas.len(), 2);
}

#[test]
fn distributed_manifest_has_two_ui_replicas() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest =
        std::fs::read_to_string(root.join("deploy/k8s/distributed/ui-deployment.yaml")).unwrap();
    assert!(manifest.contains("replicas: 2"));
    assert!(manifest.contains("maxio-ui"));
}
