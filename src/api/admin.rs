//! Authenticated admin HTTP API (P2-13).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use serde::Serialize;
use serde_json::json;

use crate::auth::principal::AuthPrincipal;
use crate::config::Config;
use crate::proxy::client_ip_from_request;
use crate::server::AppState;
use crate::storage::BucketMeta;
use crate::storage::keys;
use crate::storage::quota::disk_space_bytes;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/status", get(status))
        .route("/info", get(info))
        .route("/doctor", get(doctor))
        .route("/keyring", get(keyring))
        .route("/buckets", get(list_buckets))
        .route("/buckets/{name}", get(head_bucket))
        .route("/housekeeping/run", post(housekeeping_run))
}

pub async fn admin_auth_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let Some(principal) =
        admin_principal(&state.config, &state.credentials, request.headers())
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "unauthorized",
                "message": "Valid Bearer admin token or Basic access/secret credentials required"
            })),
        )
            .into_response();
    };
    let mut request = request;
    request.extensions_mut().insert(AuthPrincipal {
        access_key: principal,
    });
    next.run(request).await
}

pub async fn admin_rate_limit_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let ip = client_ip_from_request(&request, &state.trusted_proxies);
    if let Some(retry) = state.admin_rate_limiter.check_and_increment(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, retry.to_string())],
            Json(json!({
                "error": "rate_limited",
                "message": "admin API rate limit exceeded",
                "retry_after_secs": retry
            })),
        )
            .into_response();
    }
    next.run(request).await
}

/// Returns the audit principal when admin auth succeeds.
fn admin_principal(
    config: &Config,
    credentials: &crate::auth::credentials::CredentialStore,
    headers: &HeaderMap,
) -> Option<String> {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())?;

    if let Some(token) = auth.strip_prefix("Bearer ")
        && !config.admin_token.is_empty()
        && token == config.admin_token
    {
        return Some("admin:bearer".into());
    }

    if let Some(encoded) = auth.strip_prefix("Basic ")
        && let Ok(decoded) = B64.decode(encoded)
        && let Ok(creds) = String::from_utf8(decoded)
        && let Some((user, pass)) = creds.split_once(':')
    {
        if let Some(cred) = credentials.lookup(user) {
            if pass == cred.secret_key {
                return Some(user.to_string());
            }
            return None;
        }
        if user == config.access_key && pass == config.secret_key {
            return Some(user.to_string());
        }
    }

    None
}

#[derive(Serialize)]
struct StatusResponse {
    healthz: &'static str,
    readyz: &'static str,
    version: &'static str,
    uptime_secs: u64,
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let readyz = match state.storage.check_readiness().await {
        Ok(()) => "ok",
        Err(_) => "unavailable",
    };
    Json(StatusResponse {
        healthz: "ok",
        readyz,
        version: crate::version::VERSION,
        uptime_secs: state.started_at.elapsed().as_secs(),
    })
}

#[derive(Serialize)]
struct DiskInfo {
    total_bytes: Option<u64>,
    free_bytes: Option<u64>,
    used_bytes: Option<u64>,
}

#[derive(Serialize)]
struct ConfigInfo {
    region: String,
    erasure_coding: bool,
    chunk_size: u64,
    parity_shards: u32,
    max_object_bytes: u64,
    min_free_disk_bytes: u64,
}

#[derive(Serialize)]
struct InfoResponse {
    data_dir: String,
    disk: DiskInfo,
    bucket_count: u64,
    object_count: u64,
    config: ConfigInfo,
}

async fn info(State(state): State<AppState>) -> Result<Json<InfoResponse>, AdminApiError> {
    let data_root = state.storage.data_root();
    let (total_bytes, free_bytes) = disk_space_bytes(data_root)
        .map(|(t, f)| (Some(t), Some(f)))
        .unwrap_or((None, None));
    let used_bytes = match (total_bytes, free_bytes) {
        (Some(t), Some(f)) => Some(t.saturating_sub(f)),
        _ => None,
    };
    let bucket_count = state.storage.list_buckets().await?.len() as u64;
    let object_count = state.storage.count_all_objects().await?;

    Ok(Json(InfoResponse {
        data_dir: state.config.data_dir.clone(),
        disk: DiskInfo {
            total_bytes,
            free_bytes,
            used_bytes,
        },
        bucket_count,
        object_count,
        config: ConfigInfo {
            region: state.config.region.clone(),
            erasure_coding: state.config.erasure_coding,
            chunk_size: state.config.chunk_size,
            parity_shards: state.config.parity_shards,
            max_object_bytes: state.config.max_object_bytes,
            min_free_disk_bytes: state.config.min_free_disk_bytes,
        },
    }))
}

#[derive(Serialize)]
struct DoctorCheck {
    name: &'static str,
    ok: bool,
    detail: String,
}

#[derive(Serialize)]
struct DoctorResponse {
    ok: bool,
    checks: Vec<DoctorCheck>,
}

async fn doctor(State(state): State<AppState>) -> Json<DoctorResponse> {
    let mut checks = Vec::new();

    let readiness = state.storage.check_readiness().await;
    let readiness_detail = match &readiness {
        Ok(()) => "data directory writable and keyring usable".into(),
        Err(msg) => msg.clone(),
    };
    checks.push(DoctorCheck {
        name: "readiness",
        ok: readiness.is_ok(),
        detail: readiness_detail,
    });

    let disk_result = state.storage.check_upload_start(None);
    checks.push(DoctorCheck {
        name: "disk_reserve",
        ok: disk_result.is_ok(),
        detail: disk_result
            .map(|()| "disk reserve satisfied".into())
            .unwrap_or_else(|e| e.to_string()),
    });

    let keyring_ok = state.storage.keyring().is_usable();
    checks.push(DoctorCheck {
        name: "keyring",
        ok: keyring_ok,
        detail: if keyring_ok {
            format!(
                "SSE-S3 keyring usable (active key id {})",
                state.storage.keyring().active_id()
            )
        } else {
            "SSE-S3 keyring has no keys".into()
        },
    });

    let ok = checks.iter().all(|c| c.ok);
    Json(DoctorResponse { ok, checks })
}

#[derive(Serialize)]
struct KeyringEntry {
    id: String,
    created_at: String,
    active: bool,
}

#[derive(Serialize)]
struct KeyringResponse {
    active_id: String,
    keys: Vec<KeyringEntry>,
}

async fn keyring(State(state): State<AppState>) -> Result<Json<KeyringResponse>, AdminApiError> {
    let mut entries: Vec<KeyringEntry> = keys::list_metadata(&state.config.data_dir)
        .await?
        .into_iter()
        .map(|m| KeyringEntry {
            id: m.id,
            created_at: m.created_at,
            active: m.active,
        })
        .collect();

    let active_id = state.storage.keyring().active_id().to_string();
    if !entries.iter().any(|e| e.id == active_id) {
        entries.push(KeyringEntry {
            id: active_id.clone(),
            created_at: "from-env".into(),
            active: true,
        });
    } else {
        for entry in &mut entries {
            entry.active = entry.id == active_id;
        }
    }

    Ok(Json(KeyringResponse {
        active_id,
        keys: entries,
    }))
}

#[derive(Serialize)]
struct BucketSummary {
    name: String,
    created_at: String,
    region: String,
    object_count: u64,
}

#[derive(Serialize)]
struct BucketsResponse {
    buckets: Vec<BucketSummary>,
}

async fn list_buckets(
    State(state): State<AppState>,
) -> Result<Json<BucketsResponse>, AdminApiError> {
    let mut buckets = Vec::new();
    for meta in state.storage.list_buckets().await? {
        let object_count = state.storage.count_bucket_objects(&meta.name).await?;
        buckets.push(BucketSummary {
            name: meta.name,
            created_at: meta.created_at,
            region: meta.region,
            object_count,
        });
    }
    Ok(Json(BucketsResponse { buckets }))
}

#[derive(Serialize)]
struct BucketDetailResponse {
    bucket: BucketMeta,
    object_count: u64,
}

async fn head_bucket(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<BucketDetailResponse>, AdminApiError> {
    let buckets = state.storage.list_buckets().await?;
    let meta = buckets
        .into_iter()
        .find(|b| b.name == name)
        .ok_or(AdminApiError::NotFound(format!(
            "bucket '{name}' not found"
        )))?;
    let object_count = state.storage.count_bucket_objects(&name).await?;
    Ok(Json(BucketDetailResponse {
        bucket: meta,
        object_count,
    }))
}

#[derive(Serialize)]
struct HousekeepingResponse {
    uploads_removed: u64,
    temp_files_removed: u64,
    stale_after_days: i64,
}

async fn housekeeping_run(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Json<HousekeepingResponse> {
    let ip = client_ip_from_request(&request, &state.trusted_proxies);
    tracing::info!(
        principal = %ip,
        action = "admin.housekeeping.run",
        "admin API: on-demand housekeeping sweep"
    );
    let stale_after = chrono::Duration::days(7);
    let (uploads_removed, temp_files_removed) = state.storage.housekeeping_sweep(stale_after).await;
    Json(HousekeepingResponse {
        uploads_removed,
        temp_files_removed,
        stale_after_days: 7,
    })
}

#[derive(Debug)]
enum AdminApiError {
    NotFound(String),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for AdminApiError {
    fn from(value: anyhow::Error) -> Self {
        Self::Internal(value)
    }
}

impl From<crate::storage::StorageError> for AdminApiError {
    fn from(value: crate::storage::StorageError) -> Self {
        Self::Internal(anyhow::Error::new(value))
    }
}

impl IntoResponse for AdminApiError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": msg })),
            )
                .into_response(),
            Self::Internal(err) => {
                tracing::warn!("admin API error: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "internal_error",
                        "message": err.to_string()
                    })),
                )
                    .into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn test_config() -> Config {
        Config {
            port: 9000,
            address: "127.0.0.1".into(),
            data_dir: "./data".into(),
            access_key: "adminuser".into(),
            secret_key: "adminpass".into(),
            region: "us-east-1".into(),
            master_key: None,
            allow_insecure_dev: false,
            secure_cookies: true,
            erasure_coding: false,
            chunk_size: 10 * 1024 * 1024,
            parity_shards: 0,
            default_buckets: String::new(),
            max_console_body_bytes: 1024 * 1024,
            max_object_bytes: 0,
            min_free_disk_bytes: 0,
            s3_rate_auth_max: 60,
            s3_rate_auth_window_secs: 300,
            s3_rate_put_max: 0,
            s3_rate_put_window_secs: 60,
            admin_token: "secret-token".into(),
            admin_rate_max: 120,
            admin_rate_window_secs: 60,
            trusted_proxies: String::new(),
            login_rate_limit_redis_url: None,
            server_host: String::new(),
            metrics_enabled: false,
            metrics_port: 0,
            audit_log: false,
        }
    }

    fn headers_with(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
        headers
    }

    fn test_credentials(config: &Config) -> crate::auth::credentials::CredentialStore {
        crate::auth::credentials::CredentialStore::from_single(
            &config.access_key,
            &config.secret_key,
        )
    }

    #[test]
    fn accepts_matching_bearer_token() {
        let config = test_config();
        assert_eq!(
            admin_principal(
                &config,
                &test_credentials(&config),
                &headers_with("Bearer secret-token")
            )
            .as_deref(),
            Some("admin:bearer")
        );
    }

    #[test]
    fn rejects_wrong_bearer_token() {
        let config = test_config();
        assert!(admin_principal(
            &config,
            &test_credentials(&config),
            &headers_with("Bearer wrong-token")
        )
        .is_none());
    }

    #[test]
    fn accepts_basic_access_secret() {
        let config = test_config();
        let encoded = B64.encode("adminuser:adminpass");
        assert_eq!(
            admin_principal(
                &config,
                &test_credentials(&config),
                &headers_with(&format!("Basic {encoded}"))
            )
            .as_deref(),
            Some("adminuser")
        );
    }

    #[test]
    fn rejects_missing_auth() {
        let config = test_config();
        assert!(admin_principal(&config, &test_credentials(&config), &HeaderMap::new()).is_none());
    }
}
