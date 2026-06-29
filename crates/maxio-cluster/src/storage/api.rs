//! HTTP handlers for storage Raft RPC and status (production multi-process wiring).

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use openraft::LogIdOptionExt;
use openraft::raft::AppendEntriesRequest;
use openraft::raft::AppendEntriesResponse;
use openraft::raft::InstallSnapshotRequest;
use openraft::raft::InstallSnapshotResponse;
use openraft::raft::VoteRequest;
use openraft::raft::VoteResponse;

use maxio_storage::raft::StorageMutation;

use crate::routing::StorageRaftStatus;
use crate::storage::StorageRaftNodeHandle;
use crate::storage::types::{MutationResponse, StorageNodeId};

#[derive(Clone)]
pub struct RaftApiState {
    pub handle: StorageRaftNodeHandle,
    pub advertise_addr: String,
}

pub fn raft_router(state: RaftApiState) -> Router {
    Router::new()
        .route("/internal/raft/status", get(raft_status))
        .route("/internal/raft/vote", post(raft_vote))
        .route("/internal/raft/append_entries", post(raft_append_entries))
        .route(
            "/internal/raft/install_snapshot",
            post(raft_install_snapshot),
        )
        .route("/internal/raft/propose", post(raft_propose))
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route("/readyz", get(raft_readyz))
        .with_state(state)
}

async fn raft_status(State(state): State<RaftApiState>) -> Json<StorageRaftStatus> {
    let id = state.handle.node_id;
    let leader = state.handle.raft.current_leader().await;
    let is_leader = leader == Some(id);
    let m = state.handle.raft.metrics().borrow().clone();
    let commit_lag = match (m.last_log_index, m.last_applied.index()) {
        (Some(last), Some(applied)) => last.saturating_sub(applied),
        _ => 0,
    };
    let quorum_ok = leader.is_some();

    Json(StorageRaftStatus {
        node_id: id,
        advertise_addr: state.advertise_addr.clone(),
        current_leader: leader,
        is_leader,
        quorum_ok,
        commit_lag,
    })
}

async fn raft_readyz(State(state): State<RaftApiState>) -> StatusCode {
    if state.handle.raft.current_leader().await.is_some() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn raft_vote(
    State(state): State<RaftApiState>,
    Json(req): Json<VoteRequest<StorageNodeId>>,
) -> Result<Json<VoteResponse<StorageNodeId>>, StatusCode> {
    state
        .handle
        .raft
        .vote(req)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn raft_append_entries(
    State(state): State<RaftApiState>,
    Json(req): Json<AppendEntriesRequest<crate::storage::types::StorageRaftConfig>>,
) -> Result<Json<AppendEntriesResponse<StorageNodeId>>, StatusCode> {
    state
        .handle
        .raft
        .append_entries(req)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn raft_propose(
    State(state): State<RaftApiState>,
    Json(mutation): Json<StorageMutation>,
) -> Result<Json<MutationResponse>, StatusCode> {
    state
        .handle
        .raft
        .client_write(mutation)
        .await
        .map(|r| Json(r.data))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn raft_install_snapshot(
    State(state): State<RaftApiState>,
    Json(req): Json<InstallSnapshotRequest<crate::storage::types::StorageRaftConfig>>,
) -> Result<Json<InstallSnapshotResponse<StorageNodeId>>, StatusCode> {
    state
        .handle
        .raft
        .install_snapshot(req)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
