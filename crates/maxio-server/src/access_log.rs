//! S3 server access logging (P3-39).

use std::time::Instant;

use axum::extract::State;
use axum::http::{Method, Uri};
use axum::response::Response;

use crate::app_state::AppState;
use crate::proxy::client_ip_from_request;
use crate::storage::filesystem::AccessLogEntry;

/// Deliver S3 access log lines to configured target buckets after each request.
pub async fn access_log_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let capture = begin_access_log(&state, &request);
    let response = next.run(request).await;
    finish_access_log(&state, capture, &response);
    response
}

/// Captured request fields for access log delivery after the handler runs.
pub struct AccessLogCapture {
    started: Instant,
    uri: String,
    client_ip: String,
    user_agent: String,
    request_id: String,
    principal: String,
    bucket: Option<String>,
    key: Option<String>,
    operation: String,
}

pub fn begin_access_log(state: &AppState, request: &axum::extract::Request) -> AccessLogCapture {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let (bucket, key, operation) = parse_s3_target(&method, &uri);
    AccessLogCapture {
        started: Instant::now(),
        uri: uri.to_string(),
        client_ip: client_ip_from_request(request, &state.trusted_proxies),
        user_agent: request
            .headers()
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string(),
        request_id: request
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string(),
        principal: request
            .extensions()
            .get::<crate::auth::principal::AuthPrincipal>()
            .map(|p| p.access_key.clone())
            .unwrap_or_else(|| "-".to_string()),
        bucket,
        key,
        operation,
    }
}

pub fn finish_access_log(state: &AppState, capture: AccessLogCapture, response: &Response) {
    let Some(bucket) = capture.bucket else {
        return;
    };
    let status = response.status().as_u16();
    let entry = AccessLogEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        source_bucket: bucket,
        remote_ip: capture.client_ip,
        requester: capture.principal,
        request_id: capture.request_id,
        operation: capture.operation,
        key: capture.key.unwrap_or_default(),
        request_uri: capture.uri,
        http_status: status,
        error_code: if status >= 400 {
            "Error".into()
        } else {
            "-".into()
        },
        bytes_sent: response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        object_size: 0,
        total_time_ms: capture.started.elapsed().as_millis() as u64,
        user_agent: capture.user_agent,
    };
    let storage = state.storage.clone();
    tokio::spawn(async move {
        if let Err(e) = storage.deliver_access_log(&entry).await {
            tracing::warn!(bucket = %entry.source_bucket, error = %e, "access log delivery failed");
        }
    });
}

fn parse_s3_target(method: &Method, uri: &Uri) -> (Option<String>, Option<String>, String) {
    let path = uri.path().trim_start_matches('/');
    if path.is_empty() {
        return (None, None, "ListBuckets".into());
    }
    let segments: Vec<&str> = path.split('/').collect();
    let bucket = segments.first().map(|s| s.to_string());
    let key = if segments.len() > 1 {
        Some(segments[1..].join("/"))
    } else {
        None
    };
    let operation: String = match (method.clone(), key.is_some()) {
        (Method::PUT, true) => "REST.PUT.OBJECT".into(),
        (Method::PUT, false) => "REST.PUT.BUCKET".into(),
        (Method::GET, true) => "REST.GET.OBJECT".into(),
        (Method::GET, false) => "REST.GET.BUCKET".into(),
        (Method::HEAD, true) => "REST.HEAD.OBJECT".into(),
        (Method::HEAD, false) => "REST.HEAD.BUCKET".into(),
        (Method::DELETE, true) => "REST.DELETE.OBJECT".into(),
        (Method::DELETE, false) => "REST.DELETE.BUCKET".into(),
        (Method::POST, true) => "REST.POST.OBJECT".into(),
        (Method::POST, false) => "REST.POST.BUCKET".into(),
        _ => format!("REST.{}", method),
    };
    (bucket, key, operation)
}
