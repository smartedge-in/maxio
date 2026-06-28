use std::collections::HashMap;

use axum::{
    Extension,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};

use super::virtual_host::{VirtualHostContext, resolve_bucket, virtual_host_object_key};

use crate::error::S3Error;
use crate::server::AppState;
use crate::storage::{
    BucketEncryptionConfig, BucketMeta, CorsRule, StorageError, is_valid_bucket_name,
};
use crate::xml::{response::to_xml, types::*};

pub async fn list_buckets(State(state): State<AppState>) -> Result<Response<Body>, S3Error> {
    let buckets = state
        .storage
        .list_buckets()
        .await
        .map_err(S3Error::internal)?;

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
    Path(bucket): Path<String>,
) -> Result<Response<Body>, S3Error> {
    validate_bucket_name(&bucket)?;

    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();

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
) -> Result<Response<Body>, S3Error> {
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
    Path(_path_bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    vhost: Option<Extension<VirtualHostContext>>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response<Body>, S3Error> {
    if let Some(key) = virtual_host_object_key(&params, vhost.as_ref()) {
        let bucket = resolve_bucket(vhost.as_ref().map(|Extension(c)| c), &_path_bucket);
        return super::object::put_object(
            State(state),
            Path((bucket, key)),
            Query(HashMap::new()),
            headers,
            body,
        )
        .await;
    }

    let bucket = resolve_bucket(vhost.as_ref().map(|Extension(c)| c), &_path_bucket);

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
    create_bucket(State(state), Path(bucket)).await
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
    if sse_algorithm != "AES256" {
        return Err(S3Error::invalid_encryption_algorithm());
    }
    let cfg = BucketEncryptionConfig { sse_algorithm };
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

fn validate_bucket_name(name: &str) -> Result<(), S3Error> {
    if is_valid_bucket_name(name) {
        Ok(())
    } else {
        Err(S3Error::invalid_bucket_name(name))
    }
}
