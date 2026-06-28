use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use chrono::{NaiveDateTime, Utc};

use crate::api::virtual_host::{
    VirtualHostContext, extract_virtual_bucket, host_header_value, object_key_from_signature_path,
    signature_path_from_request,
};
use crate::error::S3Error;
use crate::proxy::client_ip_from_request;
use crate::server::AppState;

use super::signature_v4;

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, S3Error> {
    let method = request.method().as_str().to_string();
    let uri = request.uri().to_string();
    let client_ip = client_ip_from_request(&request, &state.trusted_proxies);

    tracing::debug!("{} {}", method, uri);

    if method == "PUT" && state.s3_rate_limiter.put_requests.is_enabled() {
        if let Some(retry_after) = state.s3_rate_limiter.put_requests.try_acquire(&client_ip) {
            return Err(S3Error::slow_down(retry_after));
        }
    }

    let query = request.uri().query().unwrap_or("").to_string();

    // Detect presigned URL by presence of X-Amz-Signature (case-insensitive per AWS).
    if signature_v4::query_has_presigned_signature(&query) {
        return handle_presigned(&state, &client_ip, &method, &query, request, next).await;
    }

    let has_auth_header = request.headers().get("authorization").is_some();

    let host = host_header_value(request.headers());

    let signature_path = request
        .extensions()
        .get::<VirtualHostContext>()
        .map(|ctx| ctx.signature_path.as_str());

    // Anonymous public-bucket access: no auth header, safe method, bucket flagged public.
    if !has_auth_header
        && is_public_bypass_allowed(
            &state,
            &method,
            request.uri().path(),
            &query,
            host.as_deref(),
            signature_path,
        )
        .await
    {
        tracing::debug!(
            "Public bucket bypass for {} {}",
            method,
            request.uri().path()
        );
        return Ok(next.run(request).await);
    }

    let auth_header = match request.headers().get("authorization") {
        Some(h) => h.to_str().map_err(|_| {
            auth_failure(
                &state,
                &client_ip,
                S3Error::access_denied("Invalid Authorization header"),
            )
        })?,
        None => {
            tracing::debug!("No Authorization header present");
            return Err(auth_failure(
                &state,
                &client_ip,
                S3Error::access_denied("Missing Authorization header"),
            ));
        }
    };

    tracing::debug!("Authorization: <redacted>");

    let parsed = signature_v4::parse_authorization_header(auth_header)
        .map_err(|e| auth_failure(&state, &client_ip, S3Error::access_denied(e)))?;

    tracing::debug!(
        "Parsed: date={}, region={}, signed_headers={:?}",
        parsed.date,
        parsed.region,
        parsed.signed_headers
    );

    let secret_key = match lookup_secret_key(&state, &parsed.access_key) {
        Ok(secret) => secret,
        Err(e) => {
            tracing::debug!("Access key mismatch");
            return Err(auth_failure(&state, &client_ip, e));
        }
    };

    if !signature_v4::constant_time_eq(parsed.region.as_bytes(), state.config.region.as_bytes()) {
        tracing::debug!("Region mismatch");
        return Err(auth_failure(
            &state,
            &client_ip,
            S3Error::access_denied("Invalid region in credential scope"),
        ));
    }

    // Validate request timestamp is within ±15 minutes (AWS SigV4 spec)
    let amz_date = request
        .headers()
        .get("x-amz-date")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if let Ok(request_time) = NaiveDateTime::parse_from_str(amz_date, "%Y%m%dT%H%M%SZ") {
        let now = Utc::now().naive_utc();
        let skew = (now - request_time).num_seconds().unsigned_abs();
        if skew > 15 * 60 {
            tracing::debug!("Request timestamp skew too large: {}s (max 900s)", skew);
            return Err(auth_failure(
                &state,
                &client_ip,
                S3Error::access_denied(
                    "RequestTimeTooSkewed: The difference between the request time and the current time is too large.",
                ),
            ));
        }
    } else {
        return Err(auth_failure(
            &state,
            &client_ip,
            S3Error::access_denied("Invalid or missing X-Amz-Date header"),
        ));
    }

    let path = signature_path_from_request(&request);

    tracing::debug!("Verifying signature for {} {} ?{}", method, path, query);

    for h in &parsed.signed_headers {
        tracing::debug!("  signed header '{}': {}", h, redact_header_value(h));
    }

    let valid = signature_v4::verify_signature(
        &method,
        &path,
        &query,
        request.headers(),
        &parsed,
        secret_key,
    );

    if !valid {
        tracing::debug!("Signature verification FAILED");
        return Err(auth_failure(
            &state,
            &client_ip,
            S3Error::signature_mismatch(),
        ));
    }

    tracing::debug!("Signature verification OK");
    let response = next.run(request).await;
    tracing::debug!("{} {} -> {}", method, uri, response.status());
    Ok(response)
}

fn lookup_secret_key<'a>(state: &'a AppState, access_key: &str) -> Result<&'a str, S3Error> {
    state
        .credentials
        .lookup(access_key)
        .map(|c| c.secret_key.as_str())
        .ok_or_else(S3Error::invalid_access_key)
}

fn redact_header_value(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "authorization"
        | "cookie"
        | "x-amz-security-token"
        | "x-amz-signature"
        | "x-amz-server-side-encryption-customer-key"
        | "x-amz-server-side-encryption-customer-key-md5"
        | "x-amz-copy-source-server-side-encryption-customer-key"
        | "x-amz-copy-source-server-side-encryption-customer-key-md5" => "<redacted>",
        _ => "<present>",
    }
}

/// Returns true when the request targets a public-bucket resource and is a safe read.
/// Bypass rules:
///   - Method must be GET, HEAD, or OPTIONS.
///   - Path must be `/{bucket}` (list) or `/{bucket}/{key}` (object).
///   - For bucket-level path: `public_list` must be true.
///   - For object path: `public_read` must be true.
///   - Query must not contain mutating sub-resources (`delete`, `uploads`, `tagging`,
///     `versioning`, `cors`, `encryption`, `policy`, `acl`).
async fn is_public_bypass_allowed(
    state: &AppState,
    method: &str,
    path: &str,
    query: &str,
    host: Option<&str>,
    signature_path: Option<&str>,
) -> bool {
    match method {
        "GET" | "HEAD" | "OPTIONS" => {}
        _ => return false,
    }

    if query_has_forbidden_public_subresource(query) {
        return false;
    }

    let (bucket, rest) = if let Some(host) = host {
        if let Some(bucket) = extract_virtual_bucket(host, &state.server_host) {
            let object_path = signature_path.unwrap_or(path);
            let rest = object_key_from_signature_path(object_path)
                .unwrap_or_else(|| object_path.trim_start_matches('/'))
                .to_string();
            (bucket, rest)
        } else if path == "/" || path.is_empty() {
            return false;
        } else {
            let trimmed = path.trim_start_matches('/');
            match trimmed.split_once('/') {
                Some((b, r)) => (b.to_string(), r.to_string()),
                None => (trimmed.to_string(), String::new()),
            }
        }
    } else {
        let trimmed = path.trim_start_matches('/');
        if trimmed.is_empty() {
            return false;
        }
        match trimmed.split_once('/') {
            Some((b, r)) => (b.to_string(), r.to_string()),
            None => (trimmed.to_string(), String::new()),
        }
    };

    if bucket.is_empty() {
        return false;
    }

    let (public_read, public_list) = match state.storage.get_bucket_public(&bucket).await {
        Ok(v) => v,
        Err(_) => return false,
    };

    if rest.is_empty() {
        // Bucket-level request: list/head bucket, location, etc.
        public_list
    } else {
        // Object-level request.
        public_read
    }
}

/// Sub-resource query keys that must never bypass authentication on GET/HEAD.
const PUBLIC_BYPASS_FORBIDDEN_QUERY_KEYS: &[&str] = &[
    "delete",
    "uploads",
    "tagging",
    "versioning",
    "cors",
    "encryption",
    "policy",
    "acl",
];

fn has_query_key(query: &str, key: &str) -> bool {
    for pair in query.split('&') {
        let name = pair.split('=').next().unwrap_or("");
        if name.eq_ignore_ascii_case(key) {
            return true;
        }
    }
    false
}

fn query_has_forbidden_public_subresource(query: &str) -> bool {
    PUBLIC_BYPASS_FORBIDDEN_QUERY_KEYS
        .iter()
        .any(|key| has_query_key(query, key))
}

fn auth_failure(state: &AppState, client_ip: &str, err: S3Error) -> S3Error {
    if state.s3_rate_limiter.auth_failures.is_enabled() {
        if let Some(retry_after) = state.s3_rate_limiter.auth_failures.try_acquire(client_ip) {
            return S3Error::slow_down(retry_after);
        }
    }
    err
}

async fn handle_presigned(
    state: &AppState,
    client_ip: &str,
    method: &str,
    query: &str,
    request: Request,
    next: Next,
) -> Result<Response, S3Error> {
    tracing::debug!("Presigned URL detected");

    let (parsed, timestamp, expires_secs) = signature_v4::parse_presigned_query(query)
        .map_err(|e| auth_failure(state, client_ip, S3Error::access_denied(e)))?;

    let secret_key = lookup_secret_key(state, &parsed.access_key)
        .map_err(|e| auth_failure(state, client_ip, e))?;

    if !signature_v4::constant_time_eq(parsed.region.as_bytes(), state.config.region.as_bytes()) {
        return Err(auth_failure(
            state,
            client_ip,
            S3Error::access_denied("Invalid region in credential scope"),
        ));
    }

    // Check expiration
    let issued_at = NaiveDateTime::parse_from_str(&timestamp, "%Y%m%dT%H%M%SZ").map_err(|_| {
        auth_failure(
            state,
            client_ip,
            S3Error::access_denied("Invalid X-Amz-Date format"),
        )
    })?;
    let expires_at = issued_at + chrono::Duration::seconds(expires_secs as i64);
    let now = Utc::now().naive_utc();

    if now > expires_at {
        tracing::debug!(
            "Presigned URL expired: issued={}, expires={}, now={}",
            issued_at,
            expires_at,
            now
        );
        return Err(auth_failure(
            state,
            client_ip,
            S3Error::expired_presigned_url(),
        ));
    }
    if issued_at > now + chrono::Duration::minutes(15) {
        return Err(auth_failure(
            state,
            client_ip,
            S3Error::access_denied("X-Amz-Date is too far in the future"),
        ));
    }

    let path = signature_path_from_request(&request);

    tracing::debug!(
        "Verifying presigned signature for {} {} ?{}",
        method,
        path,
        query
    );

    let valid = signature_v4::verify_presigned_signature(
        method,
        &path,
        query,
        request.headers(),
        &parsed,
        &timestamp,
        secret_key,
    );

    if !valid {
        tracing::debug!("Presigned signature verification FAILED");
        return Err(auth_failure(
            state,
            client_ip,
            S3Error::signature_mismatch(),
        ));
    }

    tracing::debug!("Presigned signature verification OK");
    let response = next.run(request).await;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_query_key_is_case_insensitive() {
        assert!(has_query_key("Policy=1&foo=bar", "policy"));
        assert!(has_query_key("POLICY", "policy"));
        assert!(!has_query_key("foo=bar", "policy"));
    }

    #[test]
    fn forbidden_subresource_queries_block_public_bypass() {
        assert!(query_has_forbidden_public_subresource("policy"));
        assert!(query_has_forbidden_public_subresource("uploads&prefix=a"));
        assert!(!query_has_forbidden_public_subresource(
            "prefix=a&delimiter=/"
        ));
    }
}
