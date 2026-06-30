use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use http::request::Parts;
use std::collections::HashMap;

use super::virtual_host::VirtualHostContext;
use crate::app_state::AppState;
use crate::auth::bucket_policy::enforce_bucket_policy_for_parts;
use crate::auth::principal::AuthPrincipal;
use crate::auth::tenant::ensure_bucket_access_optional;
use crate::error::S3Error;

/// Auth principal and virtual-host metadata from request extensions (one extractor slot).
#[derive(Clone, Debug, Default)]
pub struct S3RequestContext {
    pub principal: Option<AuthPrincipal>,
    pub vhost: Option<VirtualHostContext>,
}

impl<S> FromRequestParts<S> for S3RequestContext
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self {
            principal: parts.extensions.get::<AuthPrincipal>().cloned(),
            vhost: parts.extensions.get::<VirtualHostContext>().cloned(),
        })
    }
}

/// Tenant gate for bucket-scoped S3 handlers (P3-29).
pub async fn enforce_tenant_bucket(
    state: &AppState,
    ctx: &S3RequestContext,
    bucket: &str,
) -> Result<(), S3Error> {
    let access_key = ctx.principal.as_ref().map(|p| p.access_key.as_str());
    ensure_bucket_access_optional(state, access_key, bucket).await
}

fn query_string(params: &HashMap<String, String>) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Tenant + bucket-policy gates for object/bucket handlers (P3-28, P3-29).
pub async fn enforce_s3_bucket_gates(
    state: &AppState,
    ctx: &S3RequestContext,
    method: &str,
    bucket: &str,
    object_key: Option<&str>,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    client_ip: &str,
) -> Result<(), S3Error> {
    enforce_tenant_bucket(state, ctx, bucket).await?;
    let path = match object_key {
        Some(key) => format!("/{bucket}/{key}"),
        None => format!("/{bucket}"),
    };
    let access_key = ctx.principal.as_ref().map(|p| p.access_key.as_str());
    enforce_bucket_policy_for_parts(
        state,
        method,
        &path,
        &query_string(params),
        headers,
        ctx.vhost.as_ref(),
        access_key,
        client_ip,
    )
    .await
}

/// Tenant + `s3:GetObject` policy gate for copy sources (P3-29 / P3-28).
pub async fn enforce_copy_source_gates(
    state: &AppState,
    ctx: &S3RequestContext,
    src_bucket: &str,
    src_key: &str,
    headers: &HeaderMap,
    client_ip: &str,
) -> Result<(), S3Error> {
    enforce_tenant_bucket(state, ctx, src_bucket).await?;
    let path = format!("/{src_bucket}/{src_key}");
    let access_key = ctx.principal.as_ref().map(|p| p.access_key.as_str());
    enforce_bucket_policy_for_parts(
        state,
        "GET",
        &path,
        "",
        headers,
        ctx.vhost.as_ref(),
        access_key,
        client_ip,
    )
    .await
}
