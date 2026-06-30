use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::Ordering;

use crate::api::console::console_router;
use crate::api::cors::cors_middleware;
use crate::api::router::s3_router;
use crate::audit::audit_middleware;
use crate::auth::middleware::auth_middleware;
use crate::embedded::ui_handler;
use crate::metrics::{metrics_handler, metrics_middleware};
use crate::storage::quota::disk_space_bytes;

pub use crate::app_state::{AppState, new_app_state};

pub const HOUSEKEEPING_INTERVAL_SECS: u64 = 3600;

/// Content-Security-Policy for all HTTP responses.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none'; img-src 'self' https: data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; form-action 'self'";

pub fn build_router(state: AppState) -> Router {
    // Request order: cors → auth (virtual-host, tenant, policy, access-log, audit) → handler.
    let s3_routes = s3_router()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            cors_middleware,
        ));

    let admin_routes = crate::api::admin::router()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            audit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::api::admin::admin_rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::api::admin::admin_auth_middleware,
        ));

    let mut router = Router::new()
        .nest("/api/admin/v1", admin_routes)
        .nest("/api", console_router(state.clone()))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz));

    if state.config.metrics_enabled {
        router = router.route("/metrics", get(metrics_handler));
    }

    if state.config.serve_ui {
        router = router
            .route("/ui", get(ui_handler))
            .route("/ui/", get(ui_handler))
            .route("/ui/{*path}", get(ui_handler));
    }

    router
        .merge(s3_routes)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            metrics_middleware,
        ))
        .layer(axum::middleware::from_fn(security_headers_middleware))
        .layer(axum::middleware::from_fn(request_id_middleware))
        .with_state(state)
}

pub fn metrics_router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(|| async { StatusCode::OK }))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            metrics_middleware,
        ))
        .with_state(state)
}

#[derive(Deserialize)]
struct HealthQuery {
    verbose: Option<u8>,
}

#[derive(Serialize)]
struct VerboseHealth {
    status: &'static str,
    uptime_secs: u64,
    readyz: &'static str,
    disk: DiskHealth,
    active_multipart_uploads: u64,
    housekeeping: HousekeepingHealth,
}

#[derive(Serialize)]
struct DiskHealth {
    total_bytes: Option<u64>,
    free_bytes: Option<u64>,
    free_percent: Option<f64>,
}

#[derive(Serialize)]
struct HousekeepingHealth {
    last_run_at: Option<i64>,
    seconds_since_last_run: Option<i64>,
    interval_secs: u64,
}

async fn healthz(State(state): State<AppState>, Query(query): Query<HealthQuery>) -> Response {
    if query.verbose != Some(1) {
        return StatusCode::OK.into_response();
    }

    let readyz = match state.storage.check_readiness().await {
        Ok(()) => "ok",
        Err(_) => "unavailable",
    };

    let (total_bytes, free_bytes) = disk_space_bytes(state.storage.data_root())
        .map(|(t, f)| (Some(t), Some(f)))
        .unwrap_or((None, None));
    let free_percent = match (total_bytes, free_bytes) {
        (Some(t), Some(f)) if t > 0 => Some((f as f64 / t as f64) * 100.0),
        _ => None,
    };

    let last_run = state.last_housekeeping_at.load(Ordering::Relaxed);
    let now = chrono::Utc::now().timestamp();
    let seconds_since = if last_run > 0 {
        Some(now.saturating_sub(last_run))
    } else {
        None
    };

    let active_multipart_uploads = state.storage.count_active_multipart_uploads().await;

    Json(VerboseHealth {
        status: "ok",
        uptime_secs: state.started_at.elapsed().as_secs(),
        readyz,
        disk: DiskHealth {
            total_bytes,
            free_bytes,
            free_percent,
        },
        active_multipart_uploads,
        housekeeping: HousekeepingHealth {
            last_run_at: if last_run > 0 { Some(last_run) } else { None },
            seconds_since_last_run: seconds_since,
            interval_secs: HOUSEKEEPING_INTERVAL_SECS,
        },
    })
    .into_response()
}

async fn readyz(State(state): State<AppState>) -> StatusCode {
    if state.config.cluster_mode {
        let quorum_ok = match &state.cluster {
            Some(cluster) => cluster.storage_quorum_ok().await,
            None => false,
        };
        if !quorum_ok {
            tracing::warn!("readiness check failed: storage quorum not ok");
            return StatusCode::SERVICE_UNAVAILABLE;
        }
    }

    match state.storage.check_readiness().await {
        Ok(()) => StatusCode::OK,
        Err(e) => {
            tracing::warn!("readiness check failed: {e}");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

async fn request_id_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut response = next.run(request).await;
    if let Ok(value) = request_id.parse() {
        response.headers_mut().insert("x-amz-request-id", value);
    }
    response
}

async fn security_headers_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers
        .entry(header::CONTENT_SECURITY_POLICY)
        .or_insert_with(|| HeaderValue::from_static(CONTENT_SECURITY_POLICY));
    headers
        .entry(header::X_CONTENT_TYPE_OPTIONS)
        .or_insert(HeaderValue::from_static("nosniff"));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("strict-origin-when-cross-origin"));
    headers
        .entry(header::X_FRAME_OPTIONS)
        .or_insert(HeaderValue::from_static("DENY"));
    headers
        .entry("permissions-policy")
        .or_insert(HeaderValue::from_static(
            "camera=(), microphone=(), geolocation=()",
        ));
    response
}
