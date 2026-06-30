use std::sync::{Mutex, OnceLock};

use axum::extract::{Request, State};
use axum::http::{Method, Uri};
use axum::response::Response;
use chrono::Utc;
use serde::Serialize;

use crate::api::virtual_host::{VirtualHostContext, extract_virtual_bucket, host_header_value};
use crate::app_state::AppState;
use crate::auth::principal::AuthPrincipal;
use crate::proxy::client_ip_from_request;

static AUDIT_CAPTURE: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static AUDIT_TEST_SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// Serializes integration tests that share [`AUDIT_CAPTURE`] across parallel workers.
pub async fn lock_audit_tests() -> tokio::sync::MutexGuard<'static, ()> {
    AUDIT_TEST_SERIAL
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

/// Enables in-memory audit capture for integration tests. Returns the shared buffer.
#[allow(dead_code)]
pub fn enable_audit_capture() -> &'static Mutex<Vec<String>> {
    AUDIT_CAPTURE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Drains captured audit JSON lines (test helper).
#[allow(dead_code)]
pub fn drain_audit_capture() -> Vec<String> {
    let Some(lock) = AUDIT_CAPTURE.get() else {
        return Vec::new();
    };
    let mut lines = lock.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *lines)
}

/// Structured audit record for mutating API actions (JSON log line).
#[derive(Debug, Serialize)]
pub struct AuditRecord {
    pub timestamp: String,
    pub source: &'static str,
    pub action: String,
    pub method: String,
    pub path: String,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub principal: String,
    pub client_ip: String,
    pub status: u16,
    pub outcome: &'static str,
}

pub fn apply_virtual_host(state: &AppState, request: Request) -> Request {
    let Some(host) = host_header_value(request.headers()) else {
        return request;
    };
    let Some(bucket) = extract_virtual_bucket(&host, &state.server_host) else {
        return request;
    };
    let signature_path = request.uri().path().to_string();
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(VirtualHostContext {
        bucket,
        signature_path,
    });
    Request::from_parts(parts, body)
}

/// Audit hook invoked from S3 auth middleware after handlers run.
pub fn finish_s3_request_audit(
    state: &AppState,
    method: &Method,
    uri: &Uri,
    principal: Option<String>,
    client_ip: &str,
    response: &Response,
) {
    if !state.config.audit_log || !is_mutating(method) {
        return;
    }
    let path = uri.path().to_string();
    let principal = principal.unwrap_or_else(|| infer_principal(&path));
    let (source, bucket, key, action) = parse_audit_target(method, uri);
    let status = response.status().as_u16();
    let outcome = if status < 400 { "success" } else { "failure" };
    let record = AuditRecord {
        timestamp: Utc::now().to_rfc3339(),
        source,
        action,
        method: method.to_string(),
        path,
        bucket,
        key,
        principal,
        client_ip: client_ip.to_string(),
        status,
        outcome,
    };
    if let Ok(line) = serde_json::to_string(&record) {
        if let Some(lock) = AUDIT_CAPTURE.get()
            && let Ok(mut lines) = lock.lock()
        {
            lines.push(line.clone());
        }
        tracing::info!(target: "maxio_audit", "{line}");
    }
}

pub async fn audit_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if !state.config.audit_log {
        return next.run(request).await;
    }

    let method = request.method().clone();
    if !is_mutating(&method) {
        return next.run(request).await;
    }

    let path = request.uri().path().to_string();
    let client_ip = client_ip_from_request(&request, &state.trusted_proxies);
    let principal = request
        .extensions()
        .get::<AuthPrincipal>()
        .map(|p| p.access_key.clone())
        .unwrap_or_else(|| infer_principal(&path));

    let (source, bucket, key, action) = parse_audit_target(&method, request.uri());

    let response = next.run(request).await;
    let status = response.status().as_u16();
    let outcome = if status < 400 { "success" } else { "failure" };

    let record = AuditRecord {
        timestamp: Utc::now().to_rfc3339(),
        source,
        action,
        method: method.to_string(),
        path,
        bucket,
        key,
        principal,
        client_ip,
        status,
        outcome,
    };

    if let Ok(line) = serde_json::to_string(&record) {
        if let Some(lock) = AUDIT_CAPTURE.get()
            && let Ok(mut lines) = lock.lock()
        {
            lines.push(line.clone());
        }
        tracing::info!(target: "maxio_audit", "{line}");
    }

    response
}

fn is_mutating(method: &Method) -> bool {
    matches!(
        *method,
        Method::PUT | Method::POST | Method::DELETE | Method::PATCH
    )
}

fn infer_principal(path: &str) -> String {
    if path.starts_with("/api/admin/") {
        "admin".into()
    } else if path.starts_with("/api/") {
        "console".into()
    } else {
        "anonymous".into()
    }
}

/// Returns `(source, bucket, key, action)`.
pub fn parse_audit_target(
    method: &Method,
    uri: &Uri,
) -> (&'static str, Option<String>, Option<String>, String) {
    let path = uri.path();
    let query = uri.query().unwrap_or("");

    if path.starts_with("/api/admin/v1/") {
        let action = format!(
            "admin:{}",
            path.strip_prefix("/api/admin/v1/").unwrap_or(path)
        );
        return ("admin", None, None, action);
    }

    if path.starts_with("/api/") {
        let action = format!("console:{method} {path}");
        return ("console", None, None, action);
    }

    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let (bucket, key) = match segments.as_slice() {
        [] => (None, None),
        [bucket] => (Some((*bucket).to_string()), None),
        [bucket, rest @ ..] => (Some((*bucket).to_string()), Some(rest.join("/"))),
    };

    let mut action = format!("s3:{method} {}", path);
    if !query.is_empty() {
        action.push('?');
        action.push_str(query);
    }

    ("s3", bucket, key.filter(|k| !k.is_empty()), action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_s3_put_object() {
        let uri: Uri = "/photos/cat.jpg".parse().unwrap();
        let (source, bucket, key, action) = parse_audit_target(&Method::PUT, &uri);
        assert_eq!(source, "s3");
        assert_eq!(bucket.as_deref(), Some("photos"));
        assert_eq!(key.as_deref(), Some("cat.jpg"));
        assert!(action.contains("PUT"));
    }

    #[test]
    fn parse_admin_housekeeping() {
        let uri: Uri = "/api/admin/v1/housekeeping/run".parse().unwrap();
        let (source, _, _, action) = parse_audit_target(&Method::POST, &uri);
        assert_eq!(source, "admin");
        assert!(action.contains("housekeeping"));
    }

    #[test]
    fn mutating_methods_only() {
        assert!(is_mutating(&Method::DELETE));
        assert!(!is_mutating(&Method::GET));
    }

    #[test]
    fn audit_record_serializes_required_fields() {
        let record = AuditRecord {
            timestamp: "2026-06-28T12:00:00Z".into(),
            source: "s3",
            action: "s3:PUT /photos/cat.jpg".into(),
            method: "PUT".into(),
            path: "/photos/cat.jpg".into(),
            bucket: Some("photos".into()),
            key: Some("cat.jpg".into()),
            principal: "maxioadmin".into(),
            client_ip: "127.0.0.1".into(),
            status: 200,
            outcome: "success",
        };
        let json: serde_json::Value = serde_json::to_value(&record).unwrap();
        assert_eq!(json["source"], "s3");
        assert_eq!(json["principal"], "maxioadmin");
        assert_eq!(json["bucket"], "photos");
        assert_eq!(json["outcome"], "success");
    }
}
