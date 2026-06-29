//! Background sync of storage Raft status into server routing snapshot (production wiring).

use std::time::Duration;

use maxio_cluster::routing::{fetch_routing_snapshot, parse_storage_peers};

use crate::server::AppState;

/// Poll storage peers and publish routing snapshots while `MAXIO_CLUSTER_MODE` is enabled.
pub async fn run_cluster_sync(state: AppState) {
    let Some(peers) = parse_storage_peers(&state.config.storage_endpoints)
        .ok()
        .filter(|p| !p.is_empty())
    else {
        tracing::warn!(
            "MAXIO_CLUSTER_MODE is enabled but MAXIO_STORAGE_ENDPOINTS is empty — /readyz will stay unavailable until endpoints are configured"
        );
        return;
    };

    let interval_secs = state.config.cluster_sync_interval_secs.max(1);
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
    tracing::info!(
        peers = peers.len(),
        interval_secs,
        "cluster routing sync started"
    );

    loop {
        ticker.tick().await;
        let mut snap = fetch_routing_snapshot(&peers).await;
        if let Some(cluster) = &state.cluster {
            let epoch = cluster.routing_epoch().await.saturating_add(1);
            snap.epoch = epoch;
            cluster.publish(snap).await;
        }
    }
}
