use axum::extract::State;
use axum::http::{Method, Uri};
use axum::response::Response;
use chrono::Utc;
use serde::Serialize;

use crate::auth::principal::AuthPrincipal;
use crate::proxy::client_ip_from_request;
use crate::server::AppState;

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
}
