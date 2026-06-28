use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::routing::get;
use std::sync::Arc;

use crate::api::console::console_router;
use crate::api::cors::cors_middleware;
use crate::api::router::s3_router;
use crate::auth::middleware::auth_middleware;
use crate::config::Config;
use crate::embedded::ui_handler;
use crate::rate_limit::{AdminRateLimiter, LoginRateLimiter, S3RateLimiter};
use std::time::Instant;
use crate::storage::filesystem::FilesystemStorage;

/// Content-Security-Policy for all HTTP responses.
///
/// Scripts are loaded only from `'self'` (bundled assets under `/ui/`). Svelte
/// component styles may be injected inline, so `style-src` retains `'unsafe-inline'`.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none'; img-src 'self' https: data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; form-action 'self'";

#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<FilesystemStorage>,
    pub config: Arc<Config>,
    pub login_rate_limiter: Arc<LoginRateLimiter>,
    pub s3_rate_limiter: Arc<S3RateLimiter>,
    pub admin_rate_limiter: Arc<AdminRateLimiter>,
    pub started_at: Instant,
}

pub fn build_router(state: AppState) -> Router {
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
            crate::api::admin::admin_rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::api::admin::admin_auth_middleware,
        ));

    Router::new()
        .nest("/api/admin/v1", admin_routes)
        .nest("/api", console_router(state.clone()))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/ui", get(ui_handler))
        .route("/ui/", get(ui_handler))
        .route("/ui/{*path}", get(ui_handler))
        .merge(s3_routes)
        .layer(axum::middleware::from_fn(security_headers_middleware))
        .layer(axum::middleware::from_fn(request_id_middleware))
        .with_state(state)
}

async fn healthz() -> StatusCode {
    StatusCode::OK
}

async fn readyz(State(state): State<AppState>) -> StatusCode {
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
    headers.entry(header::CONTENT_SECURITY_POLICY).or_insert_with(|| {
        HeaderValue::from_static(CONTENT_SECURITY_POLICY)
    });
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
