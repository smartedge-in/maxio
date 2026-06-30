//! Bucket policy enforcement for authenticated S3 requests (P3-28).

use axum::http::Request as HttpRequest;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use maxio_storage::policy::{PolicyContext, PolicyDecision, PolicyRequest};

use crate::api::virtual_host::{
    VirtualHostContext, extract_virtual_bucket, host_header_value, object_key_from_signature_path,
};
use crate::app_state::AppState;
use crate::error::S3Error;
use crate::proxy::client_ip_from_request;

use super::principal::AuthPrincipal;

/// Post-auth bucket policy layer (runs after `auth_middleware` sets `AuthPrincipal`).
pub async fn bucket_policy_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, S3Error> {
    let client_ip = client_ip_from_request(&request, &state.trusted_proxies);
    let principal_key = request
        .extensions()
        .get::<AuthPrincipal>()
        .map(|p| p.access_key.clone());
    enforce_bucket_policy_if_present(&state, &request, principal_key.as_deref(), &client_ip)
        .await?;
    Ok(next.run(request).await)
}

/// Resolved bucket-scoped S3 operation for policy evaluation.
#[derive(Debug, Clone)]
pub struct BucketPolicyTarget {
    pub bucket: String,
    pub object_key: Option<String>,
    pub action: String,
    pub list_prefix: Option<String>,
}

/// Policy gate for handlers that already extracted path/query/headers (avoids `Request` extractor).
pub async fn enforce_bucket_policy_for_parts(
    state: &AppState,
    method: &str,
    path: &str,
    query: &str,
    headers: &axum::http::HeaderMap,
    vhost: Option<&VirtualHostContext>,
    principal_access_key: Option<&str>,
    client_ip: &str,
) -> Result<(), S3Error> {
    let signature_path = vhost.map(|ctx| ctx.signature_path.as_str());
    let Some(target) =
        resolve_policy_target_from_parts(method, path, query, headers, signature_path, state)
    else {
        return Ok(());
    };
    enforce_policy_target(state, &target, principal_access_key, client_ip).await
}

/// Sync policy target resolution (call before `.await` in middleware).
pub fn bucket_policy_target_for_request<B>(
    request: &HttpRequest<B>,
    state: &AppState,
) -> Option<BucketPolicyTarget> {
    resolve_policy_target(request, state)
}

/// Evaluate v2 policy for a pre-resolved target.
pub async fn enforce_bucket_policy_for_target(
    state: &AppState,
    target: Option<BucketPolicyTarget>,
    principal_access_key: Option<&str>,
    client_ip: &str,
) -> Result<(), S3Error> {
    let Some(target) = target else {
        return Ok(());
    };
    enforce_policy_target(state, &target, principal_access_key, client_ip).await
}

/// When the request targets a bucket resource, evaluate v2 policy if one is configured.
pub async fn enforce_bucket_policy_if_present<B>(
    state: &AppState,
    request: &HttpRequest<B>,
    principal_access_key: Option<&str>,
    client_ip: &str,
) -> Result<(), S3Error> {
    let target = bucket_policy_target_for_request(request, state);
    enforce_bucket_policy_for_target(state, target, principal_access_key, client_ip).await
}

async fn enforce_policy_target(
    state: &AppState,
    target: &BucketPolicyTarget,
    principal_access_key: Option<&str>,
    client_ip: &str,
) -> Result<(), S3Error> {
    let policy = match state.storage.get_bucket_policy(&target.bucket).await {
        Ok(Some(p)) => p,
        Ok(None) => return Ok(()),
        Err(crate::storage::StorageError::NotFound(_)) => return Ok(()),
        Err(e) => {
            tracing::warn!(
                bucket = %target.bucket,
                error = %e,
                "bucket policy read failed; denying request"
            );
            return Err(S3Error::internal(e));
        }
    };

    if !maxio_storage::policy::policy_requires_v2_enforcement(&target.bucket, &policy) {
        return Ok(());
    }

    let ctx = policy_context_for_principal(state, principal_access_key, client_ip);

    let req = PolicyRequest {
        action: target.action.clone(),
        bucket: target.bucket.clone(),
        object_key: target.object_key.clone(),
        list_prefix: target.list_prefix.clone(),
    };

    let decision = maxio_storage::policy::evaluate_policy_v2(&policy, &req, &ctx)
        .map_err(|e| S3Error::malformed_policy(e))?;

    match decision {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Deny | PolicyDecision::ImplicitDeny => {
            Err(S3Error::access_denied("Access Denied"))
        }
    }
}

fn resolve_policy_target<B>(
    request: &HttpRequest<B>,
    state: &AppState,
) -> Option<BucketPolicyTarget> {
    let signature_path = request
        .extensions()
        .get::<VirtualHostContext>()
        .map(|ctx| ctx.signature_path.as_str());
    resolve_policy_target_from_parts(
        request.method().as_str(),
        request.uri().path(),
        request.uri().query().unwrap_or(""),
        request.headers(),
        signature_path,
        state,
    )
}

fn resolve_policy_target_from_parts(
    method: &str,
    path: &str,
    query: &str,
    headers: &axum::http::HeaderMap,
    signature_path: Option<&str>,
    state: &AppState,
) -> Option<BucketPolicyTarget> {
    if is_create_bucket_request(method, query, path) {
        return None;
    }

    let host = host_header_value(headers);

    let (bucket, object_key) =
        extract_bucket_and_key(path, host.as_deref(), signature_path, state)?;

    let list_prefix = query
        .split('&')
        .find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k.eq_ignore_ascii_case("prefix") {
                Some(
                    percent_encoding::percent_decode_str(v)
                        .decode_utf8_lossy()
                        .into_owned(),
                )
            } else {
                None
            }
        })
        .filter(|p| !p.is_empty());

    let action = s3_action_for_request(method, query, object_key.is_some())?;

    Some(BucketPolicyTarget {
        bucket,
        object_key,
        action,
        list_prefix,
    })
}

fn extract_bucket_and_key(
    path: &str,
    host: Option<&str>,
    signature_path: Option<&str>,
    state: &AppState,
) -> Option<(String, Option<String>)> {
    if let Some(host) = host {
        if let Some(bucket) = extract_virtual_bucket(host, &state.server_host) {
            let object_path = signature_path.unwrap_or(path);
            let key = object_key_from_signature_path(object_path).map(str::to_string);
            return Some((bucket, key));
        }
    }

    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.split_once('/') {
        Some((bucket, rest)) => {
            let key = if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            };
            Some((bucket.to_string(), key))
        }
        None => Some((trimmed.to_string(), None)),
    }
}

fn s3_action_for_request(method: &str, query: &str, has_object_key: bool) -> Option<String> {
    if query_has_key(query, "uploadId") || query_has_key(query, "uploads") {
        return Some("s3:PutObject".into());
    }

    match method {
        "GET" | "HEAD" if has_object_key => Some("s3:GetObject".into()),
        "GET" | "HEAD"
            if query_has_key(query, "location")
                || query_has_key(query, "versioning")
                || query_has_key(query, "encryption")
                || query_has_key(query, "cors")
                || query_has_key(query, "lifecycle")
                || query_has_key(query, "policy")
                || query_has_key(query, "acl")
                || query_has_key(query, "tagging") =>
        {
            Some("s3:ListBucket".into())
        }
        "GET" | "HEAD" => Some("s3:ListBucket".into()),
        "PUT" if has_object_key => Some("s3:PutObject".into()),
        "PUT" => Some("s3:PutObject".into()),
        "DELETE" if has_object_key => Some("s3:DeleteObject".into()),
        "DELETE" => Some("s3:DeleteObject".into()),
        "POST" if query_has_key(query, "delete") => Some("s3:DeleteObject".into()),
        "POST" if query_has_key(query, "uploads") => Some("s3:PutObject".into()),
        _ => None,
    }
}

/// Build policy evaluation context from the authenticated principal (P3-38).
pub fn policy_context_for_principal(
    state: &AppState,
    principal_access_key: Option<&str>,
    client_ip: &str,
) -> PolicyContext {
    let (jwt_groups, jwt_roles) = principal_access_key
        .and_then(|key| state.credentials.lookup(key))
        .map(|cred| (cred.jwt_groups.clone(), cred.jwt_roles.clone()))
        .unwrap_or_default();

    PolicyContext {
        principal_access_key: principal_access_key.map(str::to_string),
        source_ip: Some(client_ip.to_string()),
        is_anonymous: principal_access_key.is_none(),
        jwt_groups,
        jwt_roles,
        oidc_claims: None,
    }
}

fn is_create_bucket_request(method: &str, query: &str, path: &str) -> bool {
    method.eq_ignore_ascii_case("PUT")
        && (query.is_empty() || !query.contains('='))
        && path
            .trim_start_matches('/')
            .split('/')
            .next()
            .is_some_and(|b| !b.is_empty())
        && !path.trim_start_matches('/').contains('/')
}

fn query_has_key(query: &str, key: &str) -> bool {
    query.split('&').any(|pair| {
        pair.split('=')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(key))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_object_put_to_put_object() {
        assert_eq!(
            s3_action_for_request("PUT", "", true).as_deref(),
            Some("s3:PutObject")
        );
    }

    #[test]
    fn maps_bucket_list_to_list_bucket() {
        assert_eq!(
            s3_action_for_request("GET", "prefix=a/", false).as_deref(),
            Some("s3:ListBucket")
        );
    }

    #[test]
    fn maps_object_delete_to_delete_object() {
        assert_eq!(
            s3_action_for_request("DELETE", "", true).as_deref(),
            Some("s3:DeleteObject")
        );
    }
}
