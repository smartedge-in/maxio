pub mod backend;
#[cfg(feature = "raft")]
pub mod raft;
#[cfg(feature = "raft-spike")]
mod raft_spike;

pub use maxio_common::cluster;
pub mod chunk_reader;
pub mod crypto;
pub mod filesystem;
pub mod keys;
pub mod kms;
pub mod metadata_index;
pub mod policy;
pub mod quota;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use tokio::io::AsyncRead;

pub type ByteStream = Pin<Box<dyn AsyncRead + Send>>;

pub fn validate_bucket_name(name: &str) -> Result<(), StorageError> {
    if is_valid_bucket_name(name) {
        Ok(())
    } else {
        Err(StorageError::InvalidKey(format!(
            "invalid bucket name: {name}"
        )))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    CRC32,
    CRC32C,
    SHA1,
    SHA256,
}

impl ChecksumAlgorithm {
    pub fn header_name(&self) -> &'static str {
        match self {
            Self::CRC32 => "x-amz-checksum-crc32",
            Self::CRC32C => "x-amz-checksum-crc32c",
            Self::SHA1 => "x-amz-checksum-sha1",
            Self::SHA256 => "x-amz-checksum-sha256",
        }
    }

    pub fn from_header_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "CRC32" => Some(Self::CRC32),
            "CRC32C" => Some(Self::CRC32C),
            "SHA1" => Some(Self::SHA1),
            "SHA256" => Some(Self::SHA256),
            _ => None,
        }
    }
}

pub struct PutResult {
    pub size: u64,
    pub etag: String,
    pub version_id: Option<String>,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub checksum_value: Option<String>,
}

pub struct DeleteResult {
    pub version_id: Option<String>,
    pub is_delete_marker: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// Prefix-based object expiration rule (P3-01 / P3-33 subset).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LifecycleRule {
    pub id: String,
    pub prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiration_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noncurrent_expiration_days: Option<u32>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObjectLockMode {
    Governance,
    Compliance,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum LegalHoldStatus {
    On,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectLockConfig {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_retention_mode: Option<ObjectLockMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_retention_days: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectLockRetention {
    pub mode: ObjectLockMode,
    pub retain_until_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BucketNotificationConfig {
    pub webhook_url: String,
    #[serde(default)]
    pub events: Vec<String>,
}

/// True when retention is active or legal hold is ON.
pub fn is_object_protected(meta: &ObjectMeta) -> bool {
    if meta.legal_hold_status == Some(LegalHoldStatus::On) {
        return true;
    }
    if let Some(ref until) = meta.retain_until_date
        && let Ok(dt) = chrono::DateTime::parse_from_rfc3339(until)
        && chrono::Utc::now() < dt.with_timezone(&chrono::Utc)
    {
        return true;
    }
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsRule {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_headers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expose_headers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketMeta {
    pub name: String,
    pub created_at: String,
    pub region: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub versioning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cors_rules: Option<Vec<CorsRule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_config: Option<BucketEncryptionConfig>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub public_read: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub public_list: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erasure_coding: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_rules: Option<Vec<LifecycleRule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging_target_bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging_target_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_config: Option<BucketNotificationConfig>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub object_lock_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_lock_config: Option<ObjectLockConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub content_type: String,
    pub last_modified: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_delete_marker: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_sizes: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_lock_mode: Option<ObjectLockMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain_until_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legal_hold_status: Option<LegalHoldStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipartUploadMeta {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
    pub content_type: String,
    pub initiated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_spec: Option<UploadEncryptionSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartMeta {
    pub part_number: u32,
    pub etag: String,
    pub size: u64,
    pub last_modified: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_value: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub encrypted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ciphertext_size: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ChunkKind {
    #[default]
    Data,
    Parity,
}

impl ChunkKind {
    fn is_data(&self) -> bool {
        *self == ChunkKind::Data
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    pub version: u32,
    pub total_size: u64,
    pub chunk_size: u64,
    pub chunk_count: u32,
    pub chunks: Vec<ChunkInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parity_shards: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plaintext_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub index: u32,
    pub size: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "ChunkKind::is_data")]
    pub kind: ChunkKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionMode {
    SseS3,
    SseC,
    SseKms,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionMeta {
    pub algorithm: String,
    pub mode: EncryptionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapped_dek: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_key_md5: Option<String>,
    pub nonce_prefix: String,
    pub chunk_size: u32,
    #[serde(default)]
    pub sidecar_mac: String,
}

pub struct EncryptionRequest {
    pub mode: EncryptionMode,
    pub customer_key: Option<zeroize::Zeroizing<[u8; 32]>>,
    pub kms_key_id: Option<String>,
}

impl EncryptionRequest {
    pub fn sse_s3() -> Self {
        Self {
            mode: EncryptionMode::SseS3,
            customer_key: None,
            kms_key_id: None,
        }
    }
    pub fn sse_c(key: [u8; 32]) -> Self {
        Self {
            mode: EncryptionMode::SseC,
            customer_key: Some(zeroize::Zeroizing::new(key)),
            kms_key_id: None,
        }
    }
    pub fn sse_kms(kms_key_id: Option<String>) -> Self {
        Self {
            mode: EncryptionMode::SseKms,
            customer_key: None,
            kms_key_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketEncryptionConfig {
    pub sse_algorithm: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadEncryptionSpec {
    pub mode: EncryptionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_key_md5: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_dek_wrapped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_dek_wrap_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_dek_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    pub upload_nonce_prefix: String,
}

pub fn is_valid_bucket_name(name: &str) -> bool {
    if name.len() < 3 || name.len() > 63 {
        return false;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return false;
    }
    let bytes = name.as_bytes();
    let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return false;
    }
    if name.contains("..") || name.contains(".-") || name.contains("-.") {
        return false;
    }
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return false;
    }
    true
}

pub async fn provision_default_buckets(
    storage: &dyn backend::StorageBackend,
    default_buckets: &str,
    region: &str,
) {
    if default_buckets.is_empty() {
        return;
    }
    for bucket_name in default_buckets.split(',') {
        let bucket_name = bucket_name.trim();
        if bucket_name.is_empty() {
            continue;
        }
        if !is_valid_bucket_name(bucket_name) {
            tracing::warn!("Skipping invalid default bucket name: '{}'", bucket_name);
            continue;
        }
        let meta = BucketMeta {
            name: bucket_name.to_string(),
            created_at: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            region: region.to_string(),
            versioning: false,
            cors_rules: None,
            encryption_config: None,
            public_read: false,
            public_list: false,
            bucket_policy: None,
            erasure_coding: None,
            lifecycle_rules: None,
            tenant_id: None,
            logging_target_bucket: None,
            logging_target_prefix: None,
            notification_config: None,
            object_lock_enabled: false,
            object_lock_config: None,
        };
        match storage.create_bucket(&meta).await {
            Ok(true) => tracing::info!("Created default bucket: {}", bucket_name),
            Ok(false) => tracing::info!("Default bucket already exists: {}", bucket_name),
            Err(e) => tracing::warn!("Failed to create default bucket '{}': {}", bucket_name, e),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Bucket not empty")]
    BucketNotEmpty,
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    #[error("Multipart upload not found: {0}")]
    UploadNotFound(String),
    #[error("Version not found: {0}")]
    VersionNotFound(String),
    #[error("Checksum mismatch: {0}")]
    ChecksumMismatch(String),
    #[error("Encryption error: {0}")]
    EncryptionError(String),
    #[error("Decryption error: {0}")]
    DecryptionError(String),
    #[error("Integrity error: {0}")]
    IntegrityError(String),
    #[error("object exceeds maximum allowed size of {max} bytes")]
    ObjectTooLarge { max: u64 },
    #[error("insufficient storage: {0}")]
    InsufficientStorage(String),
    #[error("object locked: {0}")]
    ObjectLocked(String),
}

#[cfg(test)]
mod validation_tests {
    use super::validate_bucket_name;

    #[test]
    fn rejects_path_like_bucket_names() {
        for name in [
            "../evil",
            "a/b",
            "ab",
            "evil..bucket",
            "Uppercase",
            "a.-b",
            "a-.b",
            "192.168.0.1",
        ] {
            assert!(
                validate_bucket_name(name).is_err(),
                "{name} should be invalid"
            );
        }
    }

    #[test]
    fn accepts_s3_style_bucket_name() {
        assert!(validate_bucket_name("prod-logs.2026").is_ok());
    }
}

#[cfg(test)]
mod crate_boundary_tests {
    use super::is_valid_bucket_name;

    #[test]
    fn public_api_has_no_http_types() {
        assert!(is_valid_bucket_name("logs-2026"));
    }
}
