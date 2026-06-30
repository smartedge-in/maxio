//! Multi-tenant bucket and credential scoping (P3-29).

use crate::app_state::AppState;
use crate::auth::credentials::{CredentialEntry, CredentialStore};
use crate::config::Config;
use crate::error::S3Error;
use crate::storage::BucketMeta;

pub const DEFAULT_TENANT: &str = "default";

/// Effective default tenant from server configuration.
pub fn configured_default_tenant(config: &Config) -> &str {
    let trimmed = config.default_tenant.trim();
    if trimmed.is_empty() {
        DEFAULT_TENANT
    } else {
        trimmed
    }
}

/// Resolve `None` or empty tenant IDs to the configured default (migration path).
pub fn effective_tenant_id<'a>(tenant_id: &'a Option<String>, default: &'a str) -> &'a str {
    tenant_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
}

pub fn credential_tenant_id<'a>(cred: &'a CredentialEntry, default: &'a str) -> &'a str {
    effective_tenant_id(&cred.tenant_id, default)
}

pub fn bucket_tenant_id<'a>(meta: &'a BucketMeta, default: &'a str) -> &'a str {
    effective_tenant_id(&meta.tenant_id, default)
}

/// Bootstrap admin (`MAXIO_ACCESS_KEY`) and bearer admin token see all tenants.
pub fn is_global_admin(access_key: &str, config: &Config) -> bool {
    access_key == "admin:bearer" || access_key == config.access_key
}

pub fn bucket_access_allowed(
    meta: &BucketMeta,
    access_key: &str,
    cred: Option<&CredentialEntry>,
    config: &Config,
) -> bool {
    if is_global_admin(access_key, config) {
        return true;
    }
    let default = configured_default_tenant(config);
    let Some(cred) = cred else {
        return false;
    };
    bucket_tenant_id(meta, default) == credential_tenant_id(cred, default)
}

pub async fn find_bucket_meta(
    state: &AppState,
    bucket_name: &str,
) -> Result<Option<BucketMeta>, S3Error> {
    match state.storage.head_bucket(bucket_name).await {
        Ok(false) => return Ok(None),
        Ok(true) => {}
        Err(e) => return Err(S3Error::internal(e)),
    }
    match state.storage.get_bucket_meta(bucket_name).await {
        Ok(meta) => Ok(Some(meta)),
        Err(crate::storage::StorageError::NotFound(_)) => Ok(None),
        Err(e) => Err(S3Error::internal(e)),
    }
}

/// Ensure the authenticated principal may access `bucket_name`.
pub async fn ensure_bucket_access(
    state: &AppState,
    access_key: &str,
    bucket_name: &str,
) -> Result<BucketMeta, S3Error> {
    let cred = state.credentials.lookup(access_key);
    match find_bucket_meta(state, bucket_name).await? {
        None => Err(S3Error::no_such_bucket(bucket_name)),
        Some(meta) if bucket_access_allowed(&meta, access_key, cred, &state.config) => Ok(meta),
        Some(_) => Err(S3Error::access_denied("Access Denied")),
    }
}

/// Skip tenant enforcement for anonymous public-bucket bypass (no principal).
pub async fn ensure_bucket_access_optional(
    state: &AppState,
    access_key: Option<&str>,
    bucket_name: &str,
) -> Result<(), S3Error> {
    if let Some(access_key) = access_key {
        ensure_bucket_access(state, access_key, bucket_name).await?;
    }
    Ok(())
}

pub fn filter_buckets_for_access(
    buckets: Vec<BucketMeta>,
    access_key: &str,
    credentials: &CredentialStore,
    config: &Config,
) -> Vec<BucketMeta> {
    if is_global_admin(access_key, config) {
        return buckets;
    }
    let default = configured_default_tenant(config);
    let cred_tenant = credentials
        .lookup(access_key)
        .map(|c| credential_tenant_id(c, default))
        .unwrap_or(default);
    buckets
        .into_iter()
        .filter(|b| bucket_tenant_id(b, default) == cred_tenant)
        .collect()
}

pub fn tenant_for_new_bucket(
    access_key: &str,
    credentials: &CredentialStore,
    config: &Config,
) -> String {
    let default = configured_default_tenant(config);
    if is_global_admin(access_key, config) {
        return default.to_string();
    }
    credentials
        .lookup(access_key)
        .map(|c| credential_tenant_id(c, default).to_string())
        .unwrap_or_else(|| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::credentials::CredentialEntry;

    fn sample_meta(name: &str, tenant: Option<&str>) -> BucketMeta {
        BucketMeta {
            name: name.to_string(),
            created_at: "2020-01-01T00:00:00.000Z".into(),
            region: "us-east-1".into(),
            versioning: false,
            cors_rules: None,
            encryption_config: None,
            public_read: false,
            public_list: false,
            bucket_policy: None,
            erasure_coding: None,
            lifecycle_rules: None,
            tenant_id: tenant.map(str::to_string),
            logging_target_bucket: None,
            logging_target_prefix: None,
            notification_config: None,
            object_lock_enabled: false,
            object_lock_config: None,
        }
    }

    fn sample_cred(tenant: Option<&str>) -> CredentialEntry {
        CredentialEntry {
            access_key: "user".into(),
            secret_key: "secret".into(),
            enabled: true,
            description: None,
            tenant_id: tenant.map(str::to_string),
            jwt_groups: Vec::new(),
            jwt_roles: Vec::new(),
        }
    }

    #[test]
    fn buckets_without_tenant_id_use_default() {
        let meta = sample_meta("b", None);
        assert_eq!(bucket_tenant_id(&meta, "default"), "default");
    }

    #[test]
    fn credentials_without_tenant_id_use_default() {
        let cred = sample_cred(None);
        assert_eq!(credential_tenant_id(&cred, "default"), "default");
    }

    #[test]
    fn tenant_mismatch_denies_access() {
        let config = Config {
            default_tenant: String::new(),
            access_key: "bootstrap".into(),
            ..minimal_config()
        };
        let meta = sample_meta("b", Some("tenant-a"));
        let cred = sample_cred(Some("tenant-b"));
        assert!(!bucket_access_allowed(&meta, "user", Some(&cred), &config));
    }

    #[test]
    fn bootstrap_admin_sees_all_tenants() {
        let config = Config {
            default_tenant: String::new(),
            access_key: "bootstrap".into(),
            ..minimal_config()
        };
        let meta = sample_meta("b", Some("other"));
        assert!(bucket_access_allowed(&meta, "bootstrap", None, &config));
    }

    fn minimal_config() -> Config {
        Config {
            port: 9000,
            address: "127.0.0.1".into(),
            data_dir: "./data".into(),
            access_key: "bootstrap".into(),
            secret_key: "secret".into(),
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
