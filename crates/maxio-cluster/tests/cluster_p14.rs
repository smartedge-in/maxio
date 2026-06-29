//! P1-14 epic acceptance tests (P1-17–P1-21, P1-24).

#![cfg(feature = "cluster-tests")]

use maxio_cluster::StorageCluster;
use maxio_cluster::harness::ClusterHarness;
use maxio_storage::raft::StorageMutation;

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
