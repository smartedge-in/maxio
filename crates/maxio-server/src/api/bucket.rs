use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};

use super::request_context::S3RequestContext;
use super::virtual_host::{resolve_bucket, virtual_host_object_key};
use crate::app_state::AppState;
use crate::auth::tenant::{filter_buckets_for_access, tenant_for_new_bucket};
use crate::error::S3Error;
use crate::storage::{
    BucketEncryptionConfig, BucketMeta, CorsRule, LifecycleRule, ObjectLockConfig, ObjectLockMode,
    StorageError, is_valid_bucket_name,
};
use crate::xml::{response::to_xml, types::*};

pub async fn list_buckets(
    State(state): State<AppState>,
    ctx: S3RequestContext,
) -> Result<Response<Body>, S3Error> {
    let principal = ctx
        .principal
        .ok_or_else(|| S3Error::access_denied("Missing principal"))?;
    let buckets = state
        .storage
        .list_buckets()
        .await
        .map_err(S3Error::internal)?;
    let buckets = filter_buckets_for_access(
        buckets,
        &principal.access_key,
        &state.credentials,
        &state.config,
    );

    let result = ListAllMyBucketsResult {
        owner: Owner {
            id: "maxio".to_string(),
            display_name: "maxio".to_string(),
        },
        buckets: Buckets {
            bucket: buckets
                .into_iter()
                .map(|b| BucketEntry {
                    name: b.name,
                    creation_date: b.created_at,
                })
                .collect(),
        },
    };

    let xml = to_xml(&result).map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

pub async fn create_bucket(
    State(state): State<AppState>,
    ctx: S3RequestContext,
    Path(bucket): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, S3Error> {
    let principal = ctx
        .principal
        .ok_or_else(|| S3Error::access_denied("Missing principal"))?;
    validate_bucket_name(&bucket)?;

    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let tenant_id = tenant_for_new_bucket(&principal.access_key, &state.credentials, &state.config);

    let object_lock_enabled = headers
        .get("x-amz-bucket-object-lock-enabled")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));

    let meta = BucketMeta {
        name: bucket.clone(),
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
        tenant_id: Some(tenant_id),
        logging_target_bucket: None,
        logging_target_prefix: None,
        notification_config: None,
        object_lock_enabled,
        object_lock_config: None,
    };

    let created = state
        .storage
        .create_bucket(&meta)
        .await
        .map_err(S3Error::internal)?;

    if !created {
        return Err(S3Error::bucket_already_owned(&bucket));
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Location", format!("/{}", bucket))
        .body(Body::empty())
        .unwrap())
}

pub async fn head_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    ctx: S3RequestContext,
) -> Result<Response<Body>, S3Error> {
    super::request_context::enforce_tenant_bucket(&state, &ctx, &bucket).await?;
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(StorageError::InvalidKey(_)) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("x-amz-bucket-region", &*state.config.region)
        .body(Body::empty())
        .unwrap())
}

pub async fn delete_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response<Body>, S3Error> {
    if params.contains_key("cors") {
        return delete_bucket_cors(state, bucket).await;
    }
    if params.contains_key("encryption") {
        return delete_bucket_encryption(state, bucket).await;
    }
    if params.contains_key("policy") {
        return delete_bucket_policy(state, bucket).await;
    }
    if params.contains_key("lifecycle") {
        return delete_bucket_lifecycle(state, bucket).await;
    }
    if params.contains_key("logging") {
        return delete_bucket_logging(state, bucket).await;
    }
    if params.contains_key("notification") {
        return delete_bucket_notification(state, bucket).await;
    }
    match state.storage.delete_bucket(&bucket).await {
        Ok(true) => Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap()),
        Ok(false) => Err(S3Error::no_such_bucket(&bucket)),
        Err(StorageError::BucketNotEmpty) => Err(S3Error::bucket_not_empty(&bucket)),
        Err(e) => Err(S3Error::internal(e)),
    }
}

pub async fn handle_bucket_put(
    State(state): State<AppState>,
    ctx: S3RequestContext,
    Path(_path_bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    let vhost_ref = ctx.vhost.as_ref();
    if let Some(key) = virtual_host_object_key(&params, vhost_ref) {
        let bucket = resolve_bucket(vhost_ref, &_path_bucket);
        return super::object::put_object(
            State(state),
            Path((bucket, key)),
            Query(HashMap::new()),
            headers,
            ctx,
            body,
        )
        .await;
    }

    let bucket = resolve_bucket(vhost_ref, &_path_bucket);

    if params.contains_key("versioning") {
        return put_bucket_versioning(State(state), Path(bucket), body).await;
    }
    if params.contains_key("cors") {
        return put_bucket_cors(state, bucket, body).await;
    }
    if params.contains_key("encryption") {
        return put_bucket_encryption(state, bucket, body).await;
    }
    if params.contains_key("policy") {
        return put_bucket_policy(state, bucket, body).await;
    }
    if params.contains_key("lifecycle") {
        return put_bucket_lifecycle(state, bucket, body).await;
    }
    if params.contains_key("erasure") {
        return put_bucket_erasure(state, bucket, body).await;
    }
    if params.contains_key("logging") {
        return put_bucket_logging(state, bucket, body).await;
    }
    if params.contains_key("notification") {
        return put_bucket_notification(state, bucket, body).await;
    }
    if params.contains_key("object-lock") {
        return put_bucket_object_lock(state, bucket, body).await;
    }
    create_bucket(State(state), ctx, Path(bucket), headers).await
}

async fn put_bucket_versioning(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .map_err(S3Error::internal)?;
    let body_str = String::from_utf8_lossy(&body_bytes);

    // Parse <VersioningConfiguration><Status>Enabled|Suspended</Status></VersioningConfiguration>
    let enabled = body_str.contains("<Status>Enabled</Status>");

    state
        .storage
        .set_versioning(&bucket, enabled)
        .await
        .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_versioning(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    let versioned = state
        .storage
        .is_versioned(&bucket)
        .await
        .map_err(S3Error::internal)?;

    let result = VersioningConfiguration {
        status: if versioned {
            Some("Enabled".to_string())
        } else {
            None
        },
    };

    let xml = to_xml(&result).map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn put_bucket_cors(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map_err(S3Error::internal)?;

    let config: CorsConfiguration = quick_xml::de::from_str(&String::from_utf8_lossy(&body_bytes))
        .map_err(|_| S3Error::malformed_xml())?;

    if config.rules.len() > 100 {
        return Err(S3Error::invalid_argument(
            "CORS configuration cannot have more than 100 rules",
        ));
    }
    for rule in &config.rules {
        if rule.allowed_origins.is_empty() || rule.allowed_methods.is_empty() {
            return Err(S3Error::malformed_xml());
        }
        for method in &rule.allowed_methods {
            match method.as_str() {
                "GET" | "PUT" | "POST" | "DELETE" | "HEAD" => {}
                _ => {
                    return Err(S3Error::invalid_argument(&format!(
                        "Invalid HTTP method in CORS rule: {}",
                        method
                    )));
                }
            }
        }
    }

    let rules: Vec<CorsRule> = config
        .rules
        .into_iter()
        .map(|r| CorsRule {
            allowed_origins: r.allowed_origins,
            allowed_methods: r.allowed_methods,
            allowed_headers: r.allowed_headers,
            expose_headers: r.expose_headers,
            max_age_seconds: r.max_age_seconds,
        })
        .collect();

    state
        .storage
        .put_bucket_cors(&bucket, rules)
        .await
        .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_cors(state: AppState, bucket: String) -> Result<Response<Body>, S3Error> {
    let rules = state
        .storage
        .get_bucket_cors(&bucket)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_bucket(&bucket),
            e => S3Error::internal(e),
        })?;

    let rules = rules.ok_or_else(S3Error::no_such_cors_configuration)?;

    let config = CorsConfiguration {
        rules: rules
            .into_iter()
            .map(|r| crate::xml::types::CorsRuleXml {
                allowed_origins: r.allowed_origins,
                allowed_methods: r.allowed_methods,
                allowed_headers: r.allowed_headers,
                expose_headers: r.expose_headers,
                max_age_seconds: r.max_age_seconds,
            })
            .collect(),
    };

    let xml = to_xml(&config).map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn delete_bucket_cors(state: AppState, bucket: String) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    state
        .storage
        .delete_bucket_cors(&bucket)
        .await
        .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

// --- Bucket default encryption ---------------------------------------------

async fn put_bucket_encryption(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let body_str = String::from_utf8_lossy(&body_bytes);

    // Minimal XML parsing: <ServerSideEncryptionConfiguration><Rule>
    //   <ApplyServerSideEncryptionByDefault>
    //     <SSEAlgorithm>AES256</SSEAlgorithm>
    //   </ApplyServerSideEncryptionByDefault>
    // </Rule></ServerSideEncryptionConfiguration>
    let sse_algorithm =
        extract_xml_tag(&body_str, "SSEAlgorithm").ok_or_else(S3Error::malformed_xml)?;
    if sse_algorithm != "AES256" && sse_algorithm != "aws:kms" {
        return Err(S3Error::invalid_encryption_algorithm());
    }
    if sse_algorithm == "aws:kms" && state.storage.kms().is_none() {
        return Err(S3Error::invalid_argument(
            "SSE-KMS requires MAXIO_KMS_MASTER_KEY",
        ));
    }
    let kms_key_id = extract_xml_tag(&body_str, "KMSMasterKeyID");
    let cfg = BucketEncryptionConfig {
        sse_algorithm,
        kms_key_id,
    };
    state
        .storage
        .put_bucket_encryption(&bucket, cfg)
        .await
        .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_encryption(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    let cfg = state
        .storage
        .get_bucket_encryption(&bucket)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(_) => S3Error::no_such_bucket(&bucket),
            e => S3Error::internal(e),
        })?;
    let cfg = cfg.ok_or_else(|| S3Error::no_such_bucket_encryption(&bucket))?;

    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <ServerSideEncryptionConfiguration>\
         <Rule><ApplyServerSideEncryptionByDefault>\
         <SSEAlgorithm>{}</SSEAlgorithm>\
         </ApplyServerSideEncryptionByDefault></Rule>\
         </ServerSideEncryptionConfiguration>",
        cfg.sse_algorithm
    );
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn delete_bucket_encryption(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }
    state
        .storage
        .delete_bucket_encryption(&bucket)
        .await
        .map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

async fn put_bucket_policy(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let policy = String::from_utf8(body_bytes.to_vec()).map_err(S3Error::internal)?;

    maxio_storage::policy::validate_policy_v2(&bucket, &policy)
        .map_err(S3Error::malformed_policy)?;

    state
        .storage
        .put_bucket_policy(&bucket, &policy)
        .await
        .map_err(map_policy_storage_error)?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_policy(state: AppState, bucket: String) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let policy = state
        .storage
        .get_bucket_policy(&bucket)
        .await
        .map_err(S3Error::internal)?;
    let policy = policy.ok_or_else(S3Error::no_such_bucket_policy)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(policy))
        .unwrap())
}

async fn delete_bucket_policy(state: AppState, bucket: String) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    match state.storage.get_bucket_policy(&bucket).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(S3Error::no_such_bucket_policy()),
        Err(e) => return Err(S3Error::internal(e)),
    }

    state
        .storage
        .delete_bucket_policy(&bucket)
        .await
        .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

async fn put_bucket_lifecycle(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let config: crate::xml::types::LifecycleConfiguration =
        quick_xml::de::from_str(&String::from_utf8_lossy(&body_bytes))
            .map_err(|_| S3Error::malformed_xml())?;

    let rules: Vec<LifecycleRule> = config
        .rules
        .into_iter()
        .map(|r| LifecycleRule {
            id: r.id,
            prefix: r.prefix,
            expiration_days: r.expiration.map(|e| e.days),
            transition_days: r.transition.map(|t| t.days),
            noncurrent_expiration_days: r.noncurrent_version_expiration.map(|e| e.days),
            enabled: r.status.eq_ignore_ascii_case("Enabled"),
        })
        .collect();

    state
        .storage
        .put_bucket_lifecycle(&bucket, rules)
        .await
        .map_err(map_lifecycle_storage_error)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_lifecycle(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let rules = state
        .storage
        .get_bucket_lifecycle(&bucket)
        .await
        .map_err(S3Error::internal)?;
    let rules = rules.ok_or_else(S3Error::no_such_lifecycle_configuration)?;

    let out = crate::xml::types::LifecycleConfigurationOut {
        rules: rules
            .into_iter()
            .map(|r| crate::xml::types::LifecycleRuleOut {
                id: r.id,
                prefix: r.prefix,
                status: if r.enabled {
                    "Enabled".into()
                } else {
                    "Disabled".into()
                },
                expiration: r
                    .expiration_days
                    .map(|days| crate::xml::types::LifecycleExpirationXml { days }),
                transition: r.transition_days.map(|days| {
                    crate::xml::types::LifecycleTransitionXml {
                        days,
                        storage_class: "GLACIER".into(),
                    }
                }),
                noncurrent_version_expiration: r
                    .noncurrent_expiration_days
                    .map(|days| crate::xml::types::LifecycleExpirationXml { days }),
            })
            .collect(),
    };

    let xml = to_xml(&out).map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn delete_bucket_lifecycle(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    match state.storage.get_bucket_lifecycle(&bucket).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(S3Error::no_such_lifecycle_configuration()),
        Err(e) => return Err(S3Error::internal(e)),
    }

    state
        .storage
        .delete_bucket_lifecycle(&bucket)
        .await
        .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

async fn put_bucket_erasure(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let body_str = String::from_utf8_lossy(&body_bytes);
    let enabled = body_str.contains("<Status>Enabled</Status>");

    state
        .storage
        .set_bucket_erasure_coding(&bucket, Some(enabled))
        .await
        .map_err(S3Error::internal)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_erasure(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let enabled = state
        .storage
        .get_bucket_erasure_coding(&bucket)
        .await
        .map_err(S3Error::internal)?;

    let result = crate::xml::types::ErasureConfiguration {
        status: if enabled {
            "Enabled".into()
        } else {
            "Disabled".into()
        },
    };
    let xml = to_xml(&result).map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

fn map_lifecycle_storage_error(err: StorageError) -> S3Error {
    match err {
        StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
        StorageError::NotFound(name) => S3Error::no_such_bucket(&name),
        other => S3Error::internal(other),
    }
}

fn map_policy_storage_error(err: StorageError) -> S3Error {
    match err {
        StorageError::InvalidKey(msg) => S3Error::malformed_policy(msg),
        StorageError::NotFound(name) => S3Error::no_such_bucket(&name),
        other => S3Error::internal(other),
    }
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim().to_string())
}

async fn put_bucket_logging(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }
    let body_bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let body_str = String::from_utf8_lossy(&body_bytes);
    let target_bucket =
        extract_xml_tag(&body_str, "TargetBucket").ok_or_else(S3Error::malformed_xml)?;
    let target_prefix = extract_xml_tag(&body_str, "TargetPrefix").unwrap_or_default();
    state
        .storage
        .put_bucket_logging(&bucket, &target_bucket, &target_prefix)
        .await
        .map_err(|e| match e {
            StorageError::NotFound(name) => S3Error::no_such_bucket(&name),
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            other => S3Error::internal(other),
        })?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_logging(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }
    let logging = state
        .storage
        .get_bucket_logging(&bucket)
        .await
        .map_err(S3Error::internal)?;
    let xml = match logging {
        Some((target, prefix)) => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <BucketLoggingStatus>\
             <LoggingEnabled>\
             <TargetBucket>{target}</TargetBucket>\
             <TargetPrefix>{prefix}</TargetPrefix>\
             </LoggingEnabled>\
             </BucketLoggingStatus>"
        ),
        None => "<?xml version=\"1.0\" encoding=\"UTF-8\"?><BucketLoggingStatus/>".to_string(),
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn delete_bucket_logging(state: AppState, bucket: String) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }
    state
        .storage
        .delete_bucket_logging(&bucket)
        .await
        .map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

async fn put_bucket_notification(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    use crate::events::validate_webhook_url;
    use crate::storage::BucketNotificationConfig;

    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }
    let body_bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let body_str = String::from_utf8_lossy(&body_bytes);
    let webhook_url = extract_xml_tag(&body_str, "Endpoint").ok_or_else(S3Error::malformed_xml)?;
    validate_webhook_url(&webhook_url, state.config.allow_external_webhooks)
        .map_err(|e| S3Error::invalid_argument(&e))?;
    let mut events = Vec::new();
    for event in ["s3:ObjectCreated:Put", "s3:ObjectRemoved:Delete"] {
        if body_str.contains(&format!("<Event>{event}</Event>")) {
            events.push(event.to_string());
        }
    }
    if events.is_empty() {
        return Err(S3Error::invalid_argument(
            "notification must specify at least one supported Event",
        ));
    }
    state
        .storage
        .put_bucket_notification(
            &bucket,
            BucketNotificationConfig {
                webhook_url,
                events,
            },
        )
        .await
        .map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_notification(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }
    let config = state
        .storage
        .get_bucket_notification(&bucket)
        .await
        .map_err(S3Error::internal)?;
    let xml = match config {
        Some(cfg) => {
            let events: String = cfg
                .events
                .iter()
                .map(|e| format!("<Event>{e}</Event>"))
                .collect();
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                 <NotificationConfiguration>\
                 <TopicConfiguration>\
                 <Endpoint>{}</Endpoint>{events}\
                 </TopicConfiguration>\
                 </NotificationConfiguration>",
                cfg.webhook_url
            )
        }
        None => {
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><NotificationConfiguration/>".to_string()
        }
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}

async fn delete_bucket_notification(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }
    state
        .storage
        .delete_bucket_notification(&bucket)
        .await
        .map_err(S3Error::internal)?;
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap())
}

fn validate_bucket_name(name: &str) -> Result<(), S3Error> {
    if is_valid_bucket_name(name) {
        Ok(())
    } else {
        Err(S3Error::invalid_bucket_name(name))
    }
}

fn parse_object_lock_mode(raw: &str) -> Result<ObjectLockMode, S3Error> {
    match raw.to_ascii_uppercase().as_str() {
        "GOVERNANCE" => Ok(ObjectLockMode::Governance),
        "COMPLIANCE" => Ok(ObjectLockMode::Compliance),
        _ => Err(S3Error::invalid_argument(
            "invalid ObjectLock retention Mode",
        )),
    }
}

async fn put_bucket_object_lock(
    state: AppState,
    bucket: String,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let body_bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .map_err(S3Error::internal)?;
    let config: ObjectLockConfiguration =
        quick_xml::de::from_str(&String::from_utf8_lossy(&body_bytes))
            .map_err(|_| S3Error::malformed_xml())?;

    let default_retention = config
        .rule
        .and_then(|r| r.default_retention)
        .map(|r| {
            let mode = parse_object_lock_mode(&r.mode)?;
            let days = r.days.or_else(|| r.years.map(|y| y.saturating_mul(365)));
            Ok((mode, days))
        })
        .transpose()?;

    let lock_config = ObjectLockConfig {
        enabled: config
            .object_lock_enabled
            .as_deref()
            .is_some_and(|v| v.eq_ignore_ascii_case("Enabled")),
        default_retention_mode: default_retention.as_ref().map(|(m, _)| *m),
        default_retention_days: default_retention.and_then(|(_, d)| d),
    };

    state
        .storage
        .put_bucket_object_lock(&bucket, lock_config)
        .await
        .map_err(|e| match e {
            StorageError::InvalidKey(msg) => S3Error::invalid_argument(&msg),
            StorageError::NotFound(name) => S3Error::no_such_bucket(&name),
            other => S3Error::internal(other),
        })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}

pub async fn get_bucket_object_lock(
    state: AppState,
    bucket: String,
) -> Result<Response<Body>, S3Error> {
    match state.storage.head_bucket(&bucket).await {
        Ok(true) => {}
        Ok(false) => return Err(S3Error::no_such_bucket(&bucket)),
        Err(e) => return Err(S3Error::internal(e)),
    }

    let config = state
        .storage
        .get_bucket_object_lock(&bucket)
        .await
        .map_err(S3Error::internal)?;

    let xml = match config {
        Some(cfg) => {
            let mode = cfg
                .default_retention_mode
                .map(|m| match m {
                    ObjectLockMode::Governance => "GOVERNANCE",
                    ObjectLockMode::Compliance => "COMPLIANCE",
                })
                .unwrap_or("GOVERNANCE");
            let days = cfg
                .default_retention_days
                .map(|d| format!("<Days>{d}</Days>"))
                .unwrap_or_default();
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                 <ObjectLockConfiguration>\
                 <ObjectLockEnabled>Enabled</ObjectLockEnabled>\
                 <Rule><DefaultRetention><Mode>{mode}</Mode>{days}</DefaultRetention></Rule>\
                 </ObjectLockConfiguration>"
            )
        }
        None => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <ObjectLockConfiguration><ObjectLockEnabled>Disabled</ObjectLockEnabled>\
             </ObjectLockConfiguration>"
            .to_string(),
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml")
        .body(Body::from(xml))
        .unwrap())
}
