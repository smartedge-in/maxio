//! Virtual-hosted-style S3 request routing (`bucket.endpoint/key`).
//!
//! Axum matches routes on the request URI before middleware runs, so virtual-hosted
//! requests (`Host: bucket.endpoint`, path `/key`) hit `/{bucket}` handlers with the
//! wrong path segment. [`VirtualHostContext`] records the real bucket and signed path;
//! handlers call [`resolve_bucket`] and optionally delegate object operations.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::app_state::AppState;
use crate::config::Config;
use crate::storage::validate_bucket_name;

/// Virtual-hosted request metadata attached by [`virtual_host_middleware`].
#[derive(Clone, Debug)]
pub struct VirtualHostContext {
    pub bucket: String,
    pub signature_path: String,
}

/// Object key from a virtual-hosted request path (`/key`), if present.
pub fn object_key_from_signature_path(signature_path: &str) -> Option<&str> {
    let key = signature_path.trim_start_matches('/');
    if key.is_empty() { None } else { Some(key) }
}

/// Bucket name for the request: virtual-host context wins over the path segment.
pub fn resolve_bucket(vhost: Option<&VirtualHostContext>, path_bucket: &str) -> String {
    vhost
        .map(|ctx| ctx.bucket.clone())
        .unwrap_or_else(|| path_bucket.to_string())
}

/// When `params` is empty and the request is a virtual-hosted object path, returns the key.
pub fn virtual_host_object_key(
    params: &std::collections::HashMap<String, String>,
    vhost: Option<&VirtualHostContext>,
) -> Option<String> {
    if !params.is_empty() {
        return None;
    }
    let ctx = vhost?;
    object_key_from_signature_path(&ctx.signature_path).map(str::to_string)
}

/// Path used for SigV4 canonical string (client path, not rewritten).
pub fn signature_path_from_request<B>(request: &Request<B>) -> String {
    request
        .extensions()
        .get::<VirtualHostContext>()
        .map(|ctx| ctx.signature_path.clone())
        .unwrap_or_else(|| request.uri().path().to_string())
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
    fn extracts_bucket_with_port_in_server_host() {
        assert_eq!(
            extract_virtual_bucket("my-bucket.localhost:9000", "localhost:9000").as_deref(),
            Some("my-bucket")
        );
    }

    #[test]
    fn path_style_host_does_not_extract() {
        assert!(extract_virtual_bucket("s3.example.com", "s3.example.com").is_none());
    }

    #[test]
    fn rejects_invalid_bucket_in_host() {
        assert!(extract_virtual_bucket("ab.s3.example.com", "s3.example.com").is_none());
        assert!(extract_virtual_bucket("my..bucket.s3.example.com", "s3.example.com").is_none());
    }

    #[test]
    fn object_key_from_signature_path_cases() {
        assert_eq!(object_key_from_signature_path("/a.txt"), Some("a.txt"));
        assert_eq!(object_key_from_signature_path("/"), None);
        assert_eq!(object_key_from_signature_path(""), None);
    }

    #[test]
    fn resolve_bucket_prefers_virtual_host() {
        let ctx = VirtualHostContext {
            bucket: "real".into(),
            signature_path: "/k".into(),
        };
        assert_eq!(resolve_bucket(Some(&ctx), "wrong"), "real");
        assert_eq!(resolve_bucket(None, "path-bucket"), "path-bucket");
    }

    #[test]
    fn effective_server_host_uses_config_override() {
        let mut cfg = test_config();
        cfg.server_host = "cdn.example.com".into();
        assert_eq!(effective_server_host(&cfg, Some(9000)), "cdn.example.com");
    }

    #[test]
    fn effective_server_host_derives_from_bind_address() {
        let mut cfg = test_config();
        cfg.address = "0.0.0.0".into();
        cfg.port = 9001;
        assert_eq!(effective_server_host(&cfg, Some(9001)), "127.0.0.1:9001");
    }

    #[test]
    fn host_header_value_reads_host() {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::HOST, "b.example.com".parse().unwrap());
        assert_eq!(
            host_header_value(&headers).as_deref(),
            Some("b.example.com")
        );
    }

    #[test]
    fn virtual_host_object_key_requires_empty_params() {
        let ctx = VirtualHostContext {
            bucket: "b".into(),
            signature_path: "/obj".into(),
        };
        let mut params = std::collections::HashMap::new();
        params.insert("versioning".into(), "1".into());
        assert!(virtual_host_object_key(&params, Some(&ctx)).is_none());
        params.clear();
        assert_eq!(
            virtual_host_object_key(&params, Some(&ctx)).as_deref(),
            Some("obj")
        );
        assert!(virtual_host_object_key(&params, None).is_none());
    }

    #[test]
    fn signature_path_from_request_uses_context() {
        let mut req = Request::get("/rewritten").body(()).unwrap();
        req.extensions_mut().insert(VirtualHostContext {
            bucket: "b".into(),
            signature_path: "/signed".into(),
        });
        assert_eq!(signature_path_from_request(&req), "/signed");
        let plain = Request::get("/path-only").body(()).unwrap();
        assert_eq!(signature_path_from_request(&plain), "/path-only");
    }

    fn test_config() -> Config {
        Config {
            port: 9000,
            address: "127.0.0.1".into(),
            data_dir: "./data".into(),
            access_key: "k".into(),
            secret_key: "s".into(),
            region: "us-east-1".into(),
            master_key: None,
            allow_insecure_dev: true,
            secure_cookies: false,
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
            admin_token: String::new(),
            admin_rate_max: 120,
            admin_rate_window_secs: 60,
            trusted_proxies: String::new(),
            login_rate_limit_redis_url: None,
            server_host: String::new(),
            serve_ui: true,
            cluster_mode: false,
            storage_endpoints: String::new(),
            cluster_sync_interval_secs: 5,
            metrics_enabled: false,
            metrics_port: 0,
            audit_log: false,
            metadata_index: false,
            keycloak_enabled: false,
            keycloak_base_url: String::new(),
            keycloak_realm: "kubenexis".into(),
            keycloak_client_id: "maxio-ui".into(),
            keycloak_client_secret: None,
            keycloak_skip_tls_verify: false,
            keycloak_jwks_url: None,
            keycloak_issuer: None,
            default_tenant: "default".into(),
            allow_external_webhooks: false,
        }
    }
}
