//! Authenticated admin HTTP API (P2-13).
//!
//! Stub routes return `501 Not Implemented` until admin auth and handlers land.

use axum::Router;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Json;
use serde_json::{json, Value};

pub fn router() -> Router {
    Router::new()
        .route("/status", get(stub))
        .route("/info", get(stub))
        .route("/doctor", get(stub))
        .route("/keyring", get(stub))
        .route("/buckets", get(stub))
        .route("/buckets/{name}", get(stub))
        .route("/housekeeping/run", post(stub))
}

async fn stub() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "not_implemented",
            "message": "Admin API stub — implement authenticated handlers in P2-13"
        })),
    )
}