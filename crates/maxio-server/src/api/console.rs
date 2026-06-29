use std::collections::BTreeSet;
use std::net::SocketAddr;

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use futures::TryStreamExt;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::audit::audit_middleware;
use crate::auth::keycloak::{KeycloakError, KeycloakTokenResponse};
use crate::auth::signature_v4;
use crate::server::AppState;
use crate::storage::filesystem::FilesystemStorage;

type HmacSha256 = Hmac<Sha256>;

const COOKIE_NAME: &str = "maxio_session";
const TOKEN_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60; // 7 days

fn credential_fingerprint(access_key: &str, secret_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(access_key.as_bytes());
    hasher.update(b":");
    hasher.update(secret_key.as_bytes());
    hex::encode(&hasher.finalize()[..4])
}

fn generate_token(access_key: &str, secret_key: &str, issued_at: i64) -> String {
    let issued_hex = format!("{:x}", issued_at);
    let fp = credential_fingerprint(access_key, secret_key);
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(format!("{}:{}:{}", access_key, issued_hex, fp).as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{}.{}.{}", issued_hex, sig, fp)
}

fn verify_token(token: &str, access_key: &str, secret_key: &str) -> bool {
    let mut parts = token.split('.');
    let Some(issued_hex) = parts.next() else {
        return false;
    };
    let Some(signature) = parts.next() else {
        return false;
    };
    let Some(fp) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    let current_fp = credential_fingerprint(access_key, secret_key);
    if !constant_time_eq(fp.as_bytes(), current_fp.as_bytes()) {
        return false;
    }

    let Ok(issued_at) = i64::from_str_radix(issued_hex, 16) else {
        return false;
    };

    let now = chrono::Utc::now().timestamp();
    if now - issued_at > TOKEN_MAX_AGE_SECS || issued_at > now + 60 {
        return false;
    }

    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(format!("{}:{}:{}", access_key, issued_hex, fp).as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    constant_time_eq(signature.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn extract_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .map(|c| c.trim())
                .find(|c| c.starts_with(&format!("{}=", COOKIE_NAME)))
                .map(|c| c[COOKIE_NAME.len() + 1..].to_string())
        })
}

fn make_cookie(value: &str, max_age: i64, secure: bool) -> String {
    make_named_cookie(COOKIE_NAME, value, max_age, secure)
}

fn make_named_cookie(name: &str, value: &str, max_age: i64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };

    format!("{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure_flag}")
}

fn cookie_secure(state: &AppState) -> bool {
    state.config.secure_cookies && !state.config.allow_insecure_dev
}

async fn verify_console_session(state: &AppState, token: &str) -> bool {
    if crate::auth::keycloak::is_legacy_console_session(token) {
        return verify_token(token, &state.config.access_key, &state.config.secret_key);
    }
    if let Some(keycloak) = &state.keycloak {
        return keycloak.validate_access_token(token).await.is_ok();
    }
    false
}

async fn console_auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let authenticated = match extract_cookie(request.headers()) {
        Some(token) => verify_console_session(&state, &token).await,
        None => false,
    };

    if !authenticated {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Not authenticated"})),
        )
            .into_response();
    }
    next.run(request).await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    access_key: String,
    secret_key: String,
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    let ip = state.trusted_proxies.client_ip(&headers, &addr);

    if let Some(retry_after) = state.login_rate_limiter.check_and_increment(&ip).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
            Json(serde_json::json!({"error": "Too many login attempts. Try again later."})),
        )
            .into_response();
    }

    let authenticated = state
        .credentials
        .lookup(&body.access_key)
        .is_some_and(|cred| {
            constant_time_eq(body.secret_key.as_bytes(), cred.secret_key.as_bytes())
        });
    if !authenticated {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid credentials"})),
        )
            .into_response();
    }

    let now = chrono::Utc::now().timestamp();
    let token = generate_token(&state.config.access_key, &state.config.secret_key, now);
    let cookie = make_cookie(
        &token,
        TOKEN_MAX_AGE_SECS,
        state.config.secure_cookies && !state.config.allow_insecure_dev,
    );

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert("Set-Cookie", cookie.parse().unwrap());

    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({"ok": true})),
    )
        .into_response()
}

pub async fn check(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let authenticated = match extract_cookie(&headers) {
        Some(token) => verify_console_session(&state, &token).await,
        None => false,
    };

    if authenticated {
        (StatusCode::OK, Json(serde_json::json!({"ok": true})))
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Not authenticated"})),
        )
    }
}

pub async fn logout(State(state): State<AppState>) -> impl IntoResponse {
    let secure = cookie_secure(&state);
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert("Set-Cookie", make_cookie("", 0, secure).parse().unwrap());
    if state.keycloak.is_some() {
        resp_headers.insert(
            "Set-Cookie",
            make_named_cookie(crate::auth::keycloak::REFRESH_COOKIE_NAME, "", 0, secure)
                .parse()
                .unwrap(),
        );
    }
    (
        StatusCode::OK,
        resp_headers,
        Json(serde_json::json!({"ok": true})),
    )
}

pub async fn keycloak_config(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(keycloak) = &state.keycloak {
        (StatusCode::OK, Json(keycloak.settings().config_response()))
    } else {
        (
            StatusCode::OK,
            Json(crate::auth::keycloak::KeycloakConfigResponse {
                enabled: false,
                realm: None,
                client_id: None,
            }),
        )
    }
}

#[derive(serde::Deserialize)]
pub struct KeycloakLoginRequest {
    pub username: String,
    pub password: String,
}

fn keycloak_error_response(err: KeycloakError) -> Response {
    match err {
        KeycloakError::InvalidCredentials => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid credentials"})),
        )
            .into_response(),
        KeycloakError::NotConfigured => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Keycloak is not configured"})),
        )
            .into_response(),
        KeycloakError::Unreachable(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": format!("Keycloak unreachable: {msg}")})),
        )
            .into_response(),
        KeycloakError::TokenEndpoint(msg) | KeycloakError::InvalidToken(msg) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": msg})),
        )
            .into_response(),
    }
}

fn set_keycloak_session_cookies(
    headers: &mut HeaderMap,
    state: &AppState,
    tokens: &KeycloakTokenResponse,
) {
    let secure = cookie_secure(state);
    let refresh_name = state
        .keycloak
        .as_ref()
        .expect("keycloak session cookies require keycloak")
        .refresh_cookie_name();
    headers.insert(
        "Set-Cookie",
        make_cookie(&tokens.access_token, tokens.expires_in, secure)
            .parse()
            .unwrap(),
    );
    headers.insert(
        "Set-Cookie",
        make_named_cookie(
            refresh_name,
            &tokens.refresh_token,
            tokens.refresh_expires_in,
            secure,
        )
        .parse()
        .unwrap(),
    );
}

pub async fn keycloak_login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<KeycloakLoginRequest>,
) -> Response {
    let Some(keycloak) = state.keycloak.clone() else {
        return keycloak_error_response(KeycloakError::NotConfigured);
    };

    let ip = state.trusted_proxies.client_ip(&headers, &addr);
    if let Some(retry_after) = state.login_rate_limiter.check_and_increment(&ip).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
            Json(serde_json::json!({"error": "Too many login attempts. Try again later."})),
        )
            .into_response();
    }

    match keycloak
        .password_login(&body.username, &body.password)
        .await
    {
        Ok(tokens) => {
            let mut resp_headers = HeaderMap::new();
            set_keycloak_session_cookies(&mut resp_headers, &state, &tokens);
            (StatusCode::OK, resp_headers, Json(tokens)).into_response()
        }
        Err(err) => keycloak_error_response(err),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeycloakRefreshRequest {
    pub refresh_token: Option<String>,
}

fn extract_refresh_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            let prefix = format!("{cookie_name}=");
            cookies
                .split(';')
                .map(|c| c.trim())
                .find(|c| c.starts_with(&prefix))
                .map(|c| c[prefix.len()..].to_string())
        })
}

pub async fn keycloak_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<KeycloakRefreshRequest>,
) -> Response {
    let Some(keycloak) = state.keycloak.clone() else {
        return keycloak_error_response(KeycloakError::NotConfigured);
    };

    let refresh_name = keycloak.refresh_cookie_name();
    let refresh_token = body
        .refresh_token
        .or_else(|| extract_refresh_cookie(&headers, refresh_name));

    let Some(refresh_token) = refresh_token.filter(|t| !t.is_empty()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "refresh token required"})),
        )
            .into_response();
    };

    match keycloak.refresh(&refresh_token).await {
        Ok(tokens) => {
            let mut resp_headers = HeaderMap::new();
            set_keycloak_session_cookies(&mut resp_headers, &state, &tokens);
            (StatusCode::OK, resp_headers, Json(tokens)).into_response()
        }
        Err(err) => keycloak_error_response(err),
    }
}

async fn console_csrf_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let mutating = matches!(
        method,
        axum::http::Method::POST
            | axum::http::Method::PUT
            | axum::http::Method::PATCH
            | axum::http::Method::DELETE
    );
    if mutating {
        let headers = request.headers();
        let host = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let origin = headers
            .get("origin")
            .and_then(|v| v.to_str().ok())
            .or_else(|| headers.get("referer").and_then(|v| v.to_str().ok()));
        if let Some(origin) = origin
            && !same_origin_host(origin, host)
            && !dev_loopback_origin_allowed(&state, origin, host)
        {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "CSRF origin check failed"})),
            )
                .into_response();
        }
    }
    let mut response = next.run(request).await;
    apply_security_headers(response.headers_mut());
    response
}

fn same_origin_host(origin_or_referer: &str, host: &str) -> bool {
    origin_host(origin_or_referer)
        .map(|h| h.eq_ignore_ascii_case(host))
        .unwrap_or(false)
}

fn dev_loopback_origin_allowed(state: &AppState, origin_or_referer: &str, host: &str) -> bool {
    state.config.allow_insecure_dev
        && origin_host(origin_or_referer)
            .map(|origin_host| is_loopback_host(origin_host) && is_loopback_host(host))
            .unwrap_or(false)
}

fn origin_host(origin_or_referer: &str) -> Option<&str> {
    origin_or_referer
        .strip_prefix("https://")
        .or_else(|| origin_or_referer.strip_prefix("http://"))
        .and_then(|rest| rest.split('/').next())
}

fn is_loopback_host(host_with_optional_port: &str) -> bool {
    let host = host_with_optional_port
        .strip_prefix('[')
        .and_then(|rest| rest.split(']').next())
        .unwrap_or_else(|| {
            host_with_optional_port
                .split(':')
                .next()
                .unwrap_or(host_with_optional_port)
        });

    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn apply_security_headers(headers: &mut HeaderMap) {
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "same-origin".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
}

pub async fn list_buckets(State(state): State<AppState>) -> impl IntoResponse {
    match state.storage.list_buckets().await {
        Ok(buckets) => {
            let list: Vec<serde_json::Value> = buckets
                .into_iter()
                .map(|b| {
                    serde_json::json!({
                        "name": b.name,
                        "createdAt": b.created_at,
                        "versioning": b.versioning,
                        "encryption": b.encryption_config.is_some(),
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "buckets": list }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateBucketRequest {
    name: String,
}

pub async fn create_bucket(
    State(state): State<AppState>,
    Json(body): Json<CreateBucketRequest>,
) -> impl IntoResponse {
    if crate::storage::validate_bucket_name(&body.name).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid bucket name"})),
        )
            .into_response();
    }
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let meta = crate::storage::BucketMeta {
        name: body.name.clone(),
        created_at: now,
        region: state.config.region.clone(),
        versioning: false,
        cors_rules: None,
        encryption_config: None,
        public_read: false,
        public_list: false,
        bucket_policy: None,
        erasure_coding: None,
        lifecycle_rules: None,
    };

    match state.storage.create_bucket(&meta).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Ok(false) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Bucket already exists"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_bucket_api(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    match state.storage.delete_bucket(&bucket).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Bucket not found"})),
        )
            .into_response(),
        Err(crate::storage::StorageError::BucketNotEmpty) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Bucket is not empty"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct ListObjectsParams {
    prefix: Option<String>,
    delimiter: Option<String>,
}

pub async fn list_objects(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<ListObjectsParams>,
) -> impl IntoResponse {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    let prefix = params.prefix.unwrap_or_default();
    let delimiter = params.delimiter.unwrap_or_else(|| "/".to_string());

    let all_objects = match state.storage.list_objects(&bucket, &prefix).await {
        Ok(objects) => objects,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    let mut files = Vec::new();
    let mut prefix_set = BTreeSet::new();

    for obj in &all_objects {
        let suffix = &obj.key[prefix.len()..];
        if let Some(pos) = suffix.find(delimiter.as_str()) {
            let common = format!("{}{}", prefix, &suffix[..pos + delimiter.len()]);
            prefix_set.insert(common);
        } else if !obj.key.ends_with('/') {
            files.push(serde_json::json!({
                "key": obj.key,
                "size": obj.size,
                "lastModified": obj.last_modified,
                "etag": obj.etag,
            }));
        }
    }

    // Determine which prefixes are empty (only contain a folder marker, no real objects)
    let mut empty_prefixes: Vec<&String> = Vec::new();
    for p in &prefix_set {
        let has_children = all_objects
            .iter()
            .any(|obj| obj.key.starts_with(p.as_str()) && obj.key != *p);
        if !has_children {
            empty_prefixes.push(p);
        }
    }

    let prefixes: Vec<&String> = prefix_set.iter().collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "files": files,
            "prefixes": prefixes,
            "emptyPrefixes": empty_prefixes,
        })),
    )
        .into_response()
}

pub async fn upload_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> impl IntoResponse {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    let declared_size = crate::api::object::parse_content_length(&headers);

    let stream = body.into_data_stream();
    let reader = tokio_util::io::StreamReader::new(stream.map_err(std::io::Error::other));

    let encryption = match state.storage.get_bucket_encryption(&bucket).await {
        Ok(Some(cfg)) => Some(crate::api::object::encryption_from_bucket_default(&cfg)),
        Ok(None) => None,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("failed to read bucket encryption: {}", e)
                })),
            )
                .into_response();
        }
    };

    match state
        .storage
        .put_object(
            &bucket,
            &key,
            content_type,
            Box::pin(reader),
            None,
            encryption,
            declared_size,
        )
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "etag": result.etag,
                "size": result.size,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_object_api(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
) -> impl IntoResponse {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Bucket not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    match state.storage.delete_object(&bucket, &key).await {
        Ok(_) => {
            if let Err(e) =
                preserve_empty_parent_folder_after_object_delete(&state.storage, &bucket, &key)
                    .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e})),
                )
                    .into_response();
            }
            (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

fn parent_folder_prefix_for_deleted_object(key: &str) -> Option<String> {
    if key.ends_with('/') {
        return None;
    }
    key.rfind('/')
        .map(|idx| key[..=idx].to_string())
        .filter(|prefix| !prefix.is_empty())
}

async fn preserve_empty_parent_folder_after_object_delete(
    storage: &FilesystemStorage,
    bucket: &str,
    key: &str,
) -> Result<(), String> {
    let Some(parent_prefix) = parent_folder_prefix_for_deleted_object(key) else {
        return Ok(());
    };

    let remaining = storage
        .list_objects(bucket, &parent_prefix)
        .await
        .map_err(|e| e.to_string())?;

    let parent_still_exists = remaining
        .iter()
        .any(|obj| obj.key == parent_prefix || obj.key.starts_with(&parent_prefix));
    if parent_still_exists {
        return Ok(());
    }

    storage
        .put_object(
            bucket,
            &parent_prefix,
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
            None,
            Some(0),
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub async fn download_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    let (reader, meta) = match state.storage.get_object(&bucket, &key, None).await {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Object not found"})),
            )
                .into_response();
        }
    };

    let filename = key.rsplit('/').next().unwrap_or(&key);
    let safe_filename = sanitize_filename(filename);
    let stream = tokio_util::io::ReaderStream::with_capacity(reader, 256 * 1024);
    let body = axum::body::Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &meta.content_type)
        .header("Content-Length", meta.size.to_string())
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", safe_filename),
        )
        .body(body)
        .unwrap()
        .into_response()
}

/// Sanitize a filename for use in Content-Disposition headers.
/// Removes characters that could enable header injection.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '"' && *c != '\\' && *c != '\r' && *c != '\n')
        .collect()
}

#[derive(serde::Deserialize)]
pub struct PresignParams {
    expires: Option<u64>,
}

pub async fn presign_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<PresignParams>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Verify object exists
    match state.storage.head_object(&bucket, &key).await {
        Ok(_) => {}
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Object not found"})),
            )
                .into_response();
        }
    }

    let expires_secs = params.expires.unwrap_or(3600).min(604800);

    // Determine the host from the request
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:9000");

    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let region = &state.config.region;
    let access_key = &state.config.access_key;

    let credential = format!("{}/{}/{}/s3/aws4_request", access_key, date_stamp, region);

    const S3_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    let encode =
        |s: &str| -> String { percent_encoding::utf8_percent_encode(s, S3_ENCODE).to_string() };

    // URI-encode each path segment per AWS SigV4 spec. The bucket/key values
    // arrive decoded from Axum's Path extractor, so we must encode them for
    // both the canonical request and the presigned URL.
    let encoded_key: String = key.split('/').map(&encode).collect::<Vec<_>>().join("/");
    let path = format!("/{}/{}", encode(&bucket), encoded_key);

    // Build query string params (sorted alphabetically, excluding Signature)
    let qs_params = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
        ("X-Amz-Credential", credential.clone()),
        ("X-Amz-Date", amz_date.clone()),
        ("X-Amz-Expires", expires_secs.to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
    ];

    let canonical_qs: String = qs_params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_headers = format!("host:{}\n", host);
    let canonical_request = format!(
        "GET\n{}\n{}\n{}\nhost\nUNSIGNED-PAYLOAD",
        path, canonical_qs, canonical_headers
    );

    let scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
    let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, scope, canonical_hash
    );

    let signing_key =
        signature_v4::derive_signing_key(&state.config.secret_key, &date_stamp, region);

    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    // Determine scheme
    let scheme = if headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "https")
        .unwrap_or(false)
    {
        "https"
    } else {
        "http"
    };

    let presigned_url = format!(
        "{}://{}{}?{}&X-Amz-Signature={}",
        scheme, host, path, canonical_qs, signature
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "url": presigned_url,
            "expiresIn": expires_secs,
        })),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct CreateFolderRequest {
    name: String,
}

pub async fn create_folder(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Json(body): Json<CreateFolderRequest>,
) -> impl IntoResponse {
    let name = body.name.trim().trim_matches('/');
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Folder name is required"})),
        )
            .into_response();
    }

    let key = format!("{}/", name);
    let encryption = match state.storage.get_bucket_encryption(&bucket).await {
        Ok(Some(cfg)) => Some(crate::api::object::encryption_from_bucket_default(&cfg)),
        Ok(None) => None,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("failed to read bucket encryption: {}", e)
                })),
            )
                .into_response();
        }
    };
    match state
        .storage
        .put_object(
            &bucket,
            &key,
            "application/x-directory",
            Box::pin(tokio::io::empty()),
            None,
            encryption,
            Some(0),
        )
        .await
    {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_versioning(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    match state.storage.is_versioned(&bucket).await {
        Ok(enabled) => (
            StatusCode::OK,
            Json(serde_json::json!({"enabled": enabled})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetVersioningRequest {
    enabled: bool,
}

pub async fn set_versioning(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Json(body): Json<SetVersioningRequest>,
) -> impl IntoResponse {
    match state.storage.set_versioning(&bucket, body.enabled).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_encryption(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    match state.storage.get_bucket_encryption(&bucket).await {
        Ok(Some(cfg)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "enabled": true,
                "algorithm": cfg.sse_algorithm,
            })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "enabled": false,
                "algorithm": null,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetEncryptionRequest {
    enabled: bool,
}

pub async fn set_encryption(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Json(body): Json<SetEncryptionRequest>,
) -> impl IntoResponse {
    let result = if body.enabled {
        let cfg = crate::storage::BucketEncryptionConfig {
            sse_algorithm: "AES256".to_string(),
        };
        state.storage.put_bucket_encryption(&bucket, cfg).await
    } else {
        state.storage.delete_bucket_encryption(&bucket).await
    };
    match result {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_public(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    match state.storage.get_bucket_public(&bucket).await {
        Ok((read, list)) => (
            StatusCode::OK,
            Json(serde_json::json!({"read": read, "list": list})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetPublicRequest {
    read: bool,
    list: bool,
}

pub async fn set_public(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Json(body): Json<SetPublicRequest>,
) -> impl IntoResponse {
    match state
        .storage
        .set_bucket_public(&bucket, body.read, body.list)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct ListVersionsParams {
    key: String,
}

pub async fn list_versions(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<ListVersionsParams>,
) -> impl IntoResponse {
    let all = match state
        .storage
        .list_object_versions(&bucket, &params.key)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    // Filter to only versions matching this exact key
    let versions: Vec<serde_json::Value> = all
        .into_iter()
        .filter(|v| v.key == params.key)
        .map(|v| {
            serde_json::json!({
                "versionId": v.version_id,
                "lastModified": v.last_modified,
                "size": v.size,
                "etag": v.etag,
                "isDeleteMarker": v.is_delete_marker,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({"versions": versions})),
    )
        .into_response()
}

pub async fn delete_version(
    State(state): State<AppState>,
    Path((bucket, version_id, key)): Path<(String, String, String)>,
) -> impl IntoResponse {
    match state
        .storage
        .delete_object_version(&bucket, &key, &version_id)
        .await
    {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn download_version(
    State(state): State<AppState>,
    Path((bucket, version_id, key)): Path<(String, String, String)>,
) -> Response {
    let (reader, meta) = match state
        .storage
        .get_object_version(&bucket, &key, &version_id, None)
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Version not found"})),
            )
                .into_response();
        }
    };

    let filename = key.rsplit('/').next().unwrap_or(&key);
    let safe_filename = sanitize_filename(filename);
    let stream = tokio_util::io::ReaderStream::with_capacity(reader, 256 * 1024);
    let body = axum::body::Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &meta.content_type)
        .header("Content-Length", meta.size.to_string())
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", safe_filename),
        )
        .body(body)
        .unwrap()
        .into_response()
}

pub fn console_router(state: AppState) -> Router<AppState> {
    let json_body_limit = DefaultBodyLimit::max(state.config.max_console_body_bytes);

    let public = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/check", get(check))
        .route("/auth/keycloak-config", get(keycloak_config))
        .route("/auth/keycloak-login", post(keycloak_login))
        .route("/auth/keycloak-refresh", post(keycloak_refresh))
        .layer(json_body_limit);

    let protected_limited = Router::new()
        .route("/auth/logout", post(logout))
        .route("/buckets", get(list_buckets))
        .route("/buckets", post(create_bucket))
        .route("/buckets/{bucket}", delete(delete_bucket_api))
        .route("/buckets/{bucket}/folders", post(create_folder))
        .route("/buckets/{bucket}/objects", get(list_objects))
        .route(
            "/buckets/{bucket}/objects/{*key}",
            delete(delete_object_api),
        )
        .route("/buckets/{bucket}/download/{*key}", get(download_object))
        .route("/buckets/{bucket}/presign/{*key}", get(presign_object))
        .route("/buckets/{bucket}/versioning", get(get_versioning))
        .route("/buckets/{bucket}/versioning", put(set_versioning))
        .route("/buckets/{bucket}/encryption", get(get_encryption))
        .route("/buckets/{bucket}/encryption", put(set_encryption))
        .route("/buckets/{bucket}/public", get(get_public))
        .route("/buckets/{bucket}/public", put(set_public))
        .route("/buckets/{bucket}/versions", get(list_versions))
        .route(
            "/buckets/{bucket}/versions/{version_id}/objects/{*key}",
            delete(delete_version),
        )
        .route(
            "/buckets/{bucket}/versions/{version_id}/download/{*key}",
            get(download_version),
        )
        .layer(json_body_limit);

    let protected_streaming =
        Router::new().route("/buckets/{bucket}/upload/{*key}", put(upload_object));

    let protected = protected_limited
        .merge(protected_streaming)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            audit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            console_csrf_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state,
            console_auth_middleware,
        ));

    public.merge(protected)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::storage::keys::Keyring;
    use crate::storage::quota::QuotaLimits;
    use crate::storage::{BucketMeta, ByteStream};

    use super::*;

    async fn test_storage(data_dir: &str) -> Result<FilesystemStorage, Box<dyn std::error::Error>> {
        let keyring = Arc::new(Keyring::load(data_dir, None).await?);
        let quota = QuotaLimits::from_config(0, 0);
        Ok(
            FilesystemStorage::new(data_dir, false, 10 * 1024 * 1024, 0, keyring, quota, false)
                .await?,
        )
    }

    async fn create_test_bucket(storage: &FilesystemStorage, bucket: &str) {
        storage
            .create_bucket(&BucketMeta {
                name: bucket.to_string(),
                created_at: "2026-05-18T00:00:00.000Z".to_string(),
                region: "us-east-1".to_string(),
                versioning: false,
                cors_rules: None,
                encryption_config: None,
                public_read: false,
                public_list: false,
                bucket_policy: None,
                erasure_coding: None,
                lifecycle_rules: None,
            })
            .await
            .unwrap();
    }

    fn bytes(data: &'static [u8]) -> ByteStream {
        Box::pin(data)
    }

    #[test]
    fn parent_folder_prefix_ignores_root_files_and_folder_markers() {
        assert_eq!(parent_folder_prefix_for_deleted_object("file.txt"), None);
        assert_eq!(parent_folder_prefix_for_deleted_object("folder/"), None);
        assert_eq!(
            parent_folder_prefix_for_deleted_object("folder/file.txt"),
            Some("folder/".to_string())
        );
        assert_eq!(
            parent_folder_prefix_for_deleted_object("a/b/file.txt"),
            Some("a/b/".to_string())
        );
    }

    #[tokio::test]
    async fn deleting_last_console_file_preserves_parent_folder_marker() {
        let temp = tempfile::tempdir().unwrap();
        let storage = test_storage(temp.path().to_str().unwrap()).await.unwrap();
        create_test_bucket(&storage, "bucket").await;

        storage
            .put_object(
                "bucket",
                "folder/file.txt",
                "text/plain",
                bytes(b"hello"),
                None,
                None,
                Some(5),
            )
            .await
            .unwrap();

        storage
            .delete_object("bucket", "folder/file.txt")
            .await
            .unwrap();
        preserve_empty_parent_folder_after_object_delete(&storage, "bucket", "folder/file.txt")
            .await
            .unwrap();

        let objects = storage.list_objects("bucket", "folder/").await.unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].key, "folder/");
        assert_eq!(objects[0].content_type, "application/x-directory");
    }

    #[tokio::test]
    async fn deleting_folder_marker_does_not_recreate_it() {
        let temp = tempfile::tempdir().unwrap();
        let storage = test_storage(temp.path().to_str().unwrap()).await.unwrap();
        create_test_bucket(&storage, "bucket").await;

        storage
            .put_object(
                "bucket",
                "folder/",
                "application/x-directory",
                Box::pin(tokio::io::empty()),
                None,
                None,
                Some(0),
            )
            .await
            .unwrap();

        storage.delete_object("bucket", "folder/").await.unwrap();
        preserve_empty_parent_folder_after_object_delete(&storage, "bucket", "folder/")
            .await
            .unwrap();

        let objects = storage.list_objects("bucket", "folder/").await.unwrap();
        assert!(objects.is_empty());
    }

    #[test]
    fn session_token_invalidates_when_credentials_change() {
        let now = chrono::Utc::now().timestamp();
        let token = generate_token("old-key", "old-secret", now);
        assert!(verify_token(&token, "old-key", "old-secret"));
        assert!(!verify_token(&token, "new-key", "old-secret"));
        assert!(!verify_token(&token, "old-key", "new-secret"));
    }

    #[test]
    fn legacy_two_part_tokens_are_rejected() {
        let now = chrono::Utc::now().timestamp();
        let issued_hex = format!("{:x}", now);
        let legacy = format!("{issued_hex}.deadbeef");
        assert!(!verify_token(&legacy, "maxioadmin", "maxioadmin"));
    }
}
