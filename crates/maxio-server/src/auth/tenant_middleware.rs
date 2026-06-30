//! Post-auth tenant boundary enforcement on bucket-scoped S3 routes (P3-29).

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use http::Method;

use crate::api::virtual_host::{VirtualHostContext, extract_virtual_bucket, host_header_value};
use crate::app_state::AppState;
use crate::error::S3Error;

use super::principal::AuthPrincipal;
use super::tenant::ensure_bucket_access_optional;

/// Enforce tenant scope after SigV4 auth for bucket-level and object-level S3 paths.
pub async fn tenant_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, S3Error> {
    enforce_tenant_scope(&state, &request).await?;
    Ok(next.run(request).await)
}

/// Sync tenant-scope plan (no `&Request` held across `.await` in callers).
#[derive(Debug, Clone)]
pub struct TenantScopePlan {
    pub access_key: Option<String>,
    pub bucket: String,
}

/// Returns a tenant check when the request targets an existing bucket resource.
pub fn plan_tenant_scope(state: &AppState, request: &Request) -> Option<TenantScopePlan> {
    let method = request.method().clone();
    let uri = request.uri();
    let query = uri.query().unwrap_or("");
    let path = uri.path();

    let access_key = request
        .extensions()
        .get::<AuthPrincipal>()
        .map(|p| p.access_key.clone());

    if path == "/" || path.is_empty() {
        return None;
    }

    let bucket = resolve_bucket_name(state, request, path)?;
    if is_create_bucket(&method, query) {
        return None;
    }

    Some(TenantScopePlan { access_key, bucket })
}

/// Tenant boundary check invoked from auth middleware (avoids an extra Axum layer).
pub async fn enforce_tenant_scope(
    state: &AppState,
    request: &Request,
) -> Result<(), crate::error::S3Error> {
    let Some(plan) = plan_tenant_scope(state, request) else {
        return Ok(());
    };
    ensure_bucket_access_optional(state, plan.access_key.as_deref(), &plan.bucket).await
}

fn resolve_bucket_name(state: &AppState, request: &Request, path: &str) -> Option<String> {
    let host = host_header_value(request.headers());
    if let Some(host) = host.as_deref() {
        if let Some(bucket) = extract_virtual_bucket(host, &state.server_host) {
            return Some(bucket);
        }
    }
    if let Some(ctx) = request.extensions().get::<VirtualHostContext>() {
        return Some(ctx.bucket.clone());
    }
    let trimmed = path.trim_start_matches('/');
    let bucket = trimmed.split('/').next()?.to_string();
    if bucket.is_empty() {
        return None;
    }
    Some(bucket)
}

fn is_create_bucket(method: &Method, query: &str) -> bool {
    if *method != Method::PUT {
        return false;
    }
    if query.is_empty() {
        return true;
    }
    !query.contains('=')
}
