//! Virtual-hosted-style S3 request routing (`bucket.endpoint/key`).

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::config::Config;
use crate::server::AppState;
use crate::storage::validate_bucket_name;

/// Virtual-hosted request metadata (Axum matches path before middleware; handlers dispatch using this).
#[derive(Clone, Debug)]
pub struct VirtualHostContext {
    pub bucket: String,
    pub signature_path: String,
}

/// Object key from a virtual-hosted request path (`/key`), if present.
pub fn virtual_host_object_key(signature_path: &str) -> Option<&str> {
    let key = signature_path.trim_start_matches('/');
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

pub fn effective_server_host(config: &Config, listen_port: Option<u16>) -> String {
    if !config.server_host.trim().is_empty() {
        return config.server_host.trim().to_string();
    }
    let host = if config.address == "0.0.0.0" {
        "127.0.0.1"
    } else {
        config.address.as_str()
    };
    let port = listen_port.unwrap_or(config.port);
    format!("{host}:{port}")
}

/// Extract bucket name when `host` is `{bucket}.{server_host}`.
pub fn extract_virtual_bucket(host: &str, server_host: &str) -> Option<String> {
    let host = host.trim().to_ascii_lowercase();
    let server_host = server_host.trim().to_ascii_lowercase();
    if host == server_host {
        return None;
    }
    let suffix = format!(".{server_host}");
    let bucket = host.strip_suffix(&suffix)?;
    if bucket.is_empty() {
        return None;
    }
    validate_bucket_name(bucket).ok()?;
    Some(bucket.to_string())
}

pub fn host_header_value(headers: &http::HeaderMap) -> Option<String> {
    headers
        .get(http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

pub fn rewrite_path(bucket: &str, path: &str) -> String {
    if path == "/" || path.is_empty() {
        format!("/{bucket}")
    } else {
        format!("/{bucket}{path}")
    }
}

pub async fn virtual_host_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(host) = host_header_value(request.headers()) else {
        return next.run(request).await;
    };

    let Some(bucket) = extract_virtual_bucket(&host, &state.server_host) else {
        return next.run(request).await;
    };

    let signature_path = request.uri().path().to_string();
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(VirtualHostContext {
        bucket,
        signature_path,
    });
    next.run(Request::from_parts(parts, body)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_bucket_from_virtual_host() {
        assert_eq!(
            extract_virtual_bucket("my-bucket.s3.example.com", "s3.example.com").as_deref(),
            Some("my-bucket")
        );
    }

    #[test]
    fn path_style_host_does_not_extract() {
        assert!(extract_virtual_bucket("s3.example.com", "s3.example.com").is_none());
    }

    #[test]
    fn rewrites_object_and_list_paths() {
        assert_eq!(rewrite_path("b", "/key.txt"), "/b/key.txt");
        assert_eq!(rewrite_path("b", "/"), "/b");
    }
}