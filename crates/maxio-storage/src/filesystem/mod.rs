use super::chunk_reader::VerifiedChunkReader;
use super::crypto::{AadBuilder, FRAME_CHUNK_SIZE, FrameDecryptor};
use super::keys::Keyring;
use super::kms::KmsBackend;
use super::metadata_index::MetadataIndex;
use super::quota::{QuotaLimits, QuotaReader, map_read_quota_error};
use super::{
    BucketEncryptionConfig, BucketMeta, BucketNotificationConfig, ByteStream, ChecksumAlgorithm,
    ChunkInfo, ChunkKind, ChunkManifest, DeleteResult, EncryptionMeta, EncryptionMode,
    EncryptionRequest, LifecycleRule, MultipartUploadMeta, ObjectMeta, PartMeta, PutResult,
    StorageError, UploadEncryptionSpec, validate_bucket_name,
};
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use base64::Engine;
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use rand::RngExt;
use sha2::Sha256;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter};

type HmacSha256 = Hmac<Sha256>;

const IO_BUFFER_SIZE: usize = 256 * 1024;
const SMALL_OBJECT_THRESHOLD: u64 = 256 * 1024;

mod access_logging;
mod common;
#[cfg(test)]
mod ec_policy_tests;
mod encryption_io;
mod housekeeping;
#[cfg(test)]
mod index_parity_tests;
mod listing;
mod multipart;
mod object_io;
mod object_lock;

use common::*;

pub use access_logging::AccessLogEntry;

enum ChecksumHasher {
    Crc32(crc32fast::Hasher),
    Crc32c(u32),
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
}

impl ChecksumHasher {
    pub(super) fn new(algo: ChecksumAlgorithm) -> Self {
        match algo {
            ChecksumAlgorithm::CRC32 => Self::Crc32(crc32fast::Hasher::new()),
            ChecksumAlgorithm::CRC32C => Self::Crc32c(0),
            ChecksumAlgorithm::SHA1 => Self::Sha1(<sha1::Sha1 as Digest>::new()),
            ChecksumAlgorithm::SHA256 => Self::Sha256(<sha2::Sha256 as Digest>::new()),
        }
    }

    pub(super) fn update(&mut self, data: &[u8]) {
        match self {
            Self::Crc32(h) => h.update(data),
            Self::Crc32c(v) => *v = crc32c::crc32c_append(*v, data),
            Self::Sha1(h) => Digest::update(h, data),
            Self::Sha256(h) => Digest::update(h, data),
        }
    }

    pub(super) fn finalize_base64(self) -> String {
        let b64 = base64::engine::general_purpose::STANDARD;
        match self {
            Self::Crc32(h) => b64.encode(h.finalize().to_be_bytes()),
            Self::Crc32c(v) => b64.encode(v.to_be_bytes()),
            Self::Sha1(h) => b64.encode(Digest::finalize(h)),
            Self::Sha256(h) => b64.encode(Digest::finalize(h)),
        }
    }
}

pub struct FilesystemStorage {
    pub(super) data_root: PathBuf,
    pub(super) buckets_dir: PathBuf,
    pub(super) erasure_coding: bool,
    pub(super) chunk_size: u64,
    pub(super) parity_shards: u32,
    pub(super) keyring: Arc<Keyring>,
    pub(super) kms: Option<Arc<dyn KmsBackend>>,
    pub(super) quota: QuotaLimits,
    pub(super) metadata_index: Option<Arc<MetadataIndex>>,
}

impl FilesystemStorage {
    pub async fn new(
        data_dir: &str,
        erasure_coding: bool,
        chunk_size: u64,
        parity_shards: u32,
        keyring: Arc<Keyring>,
        kms: Option<Arc<dyn KmsBackend>>,
        quota: QuotaLimits,
        metadata_index_enabled: bool,
    ) -> Result<Self, anyhow::Error> {
        let data_root = Path::new(data_dir).to_path_buf();
        let buckets_dir = data_root.join("buckets");
        fs::create_dir_all(&buckets_dir).await?;
        let metadata_index = if metadata_index_enabled {
            Some(Arc::new(MetadataIndex::open(&data_root)?))
        } else {
            None
        };
        let storage = Self {
            data_root,
            buckets_dir,
            erasure_coding,
            chunk_size,
            parity_shards,
            keyring,
            kms,
            quota,
            metadata_index,
        };
        if metadata_index_enabled {
            storage.rebuild_all_metadata_indexes().await?;
        }
        Ok(storage)
    }

    pub fn metadata_index_enabled(&self) -> bool {
        self.metadata_index.is_some()
    }

    pub(super) fn index_upsert(&self, bucket: &str, meta: &ObjectMeta) {
        if let Some(index) = &self.metadata_index
            && let Err(err) = index.upsert(bucket, meta)
        {
            tracing::warn!(
                "metadata index upsert failed for {bucket}/{}: {err}",
                meta.key
            );
        }
    }

    pub(super) fn index_remove(&self, bucket: &str, key: &str) {
        if let Some(index) = &self.metadata_index
            && let Err(err) = index.remove(bucket, key)
        {
            tracing::warn!("metadata index remove failed for {bucket}/{key}: {err}");
        }
    }

    pub async fn rebuild_all_metadata_indexes(&self) -> Result<(), StorageError> {
        let Some(index) = &self.metadata_index else {
            return Ok(());
        };
        for bucket in self.list_buckets().await? {
            let objects = self.list_objects_walk(&bucket.name, "").await?;
            index.rebuild_bucket(&bucket.name, &objects)?;
            tracing::info!(
                "metadata index: rebuilt {} objects for bucket {}",
                objects.len(),
                bucket.name
            );
        }
        Ok(())
    }

    pub async fn effective_erasure_coding(&self, bucket: &str) -> Result<bool, StorageError> {
        if !self.erasure_coding {
            return Ok(false);
        }
        let meta = self.read_bucket_meta(bucket).await?;
        Ok(meta.erasure_coding.unwrap_or(true))
    }

    pub async fn get_bucket_meta(&self, bucket: &str) -> Result<BucketMeta, StorageError> {
        self.read_bucket_meta(bucket).await
    }

    pub(super) async fn read_bucket_meta(&self, bucket: &str) -> Result<BucketMeta, StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    pub async fn set_bucket_erasure_coding(
        &self,
        bucket: &str,
        enabled: Option<bool>,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        if enabled == Some(true) && !self.erasure_coding {
            return Err(StorageError::InvalidKey(
                "per-bucket erasure coding requires server-wide MAXIO_ERASURE_CODING".into(),
            ));
        }
        let mut meta = self.read_bucket_meta(bucket).await?;
        meta.erasure_coding = enabled;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn get_bucket_erasure_coding(&self, bucket: &str) -> Result<bool, StorageError> {
        self.effective_erasure_coding(bucket).await
    }

    pub async fn put_bucket_lifecycle(
        &self,
        bucket: &str,
        rules: Vec<LifecycleRule>,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        for rule in &rules {
            if rule.id.is_empty() {
                return Err(StorageError::InvalidKey(
                    "lifecycle rule id must not be empty".into(),
                ));
            }
            if rule.expiration_days == Some(0) {
                return Err(StorageError::InvalidKey(
                    "lifecycle rule expiration_days must be > 0".into(),
                ));
            }
        }
        let mut meta = self.read_bucket_meta(bucket).await?;
        meta.lifecycle_rules = Some(rules);
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn get_bucket_lifecycle(
        &self,
        bucket: &str,
    ) -> Result<Option<Vec<LifecycleRule>>, StorageError> {
        Ok(self.read_bucket_meta(bucket).await?.lifecycle_rules)
    }

    pub async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let mut meta = self.read_bucket_meta(bucket).await?;
        meta.lifecycle_rules = None;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn kms_backend(&self) -> Option<&Arc<dyn KmsBackend>> {
        self.kms.as_ref()
    }

    pub fn keyring(&self) -> &Arc<Keyring> {
        &self.keyring
    }

    pub async fn check_readiness(&self) -> Result<(), String> {
        if !fs::try_exists(&self.data_root)
            .await
            .map_err(|e| format!("data directory stat failed: {e}"))?
        {
            return Err("data directory missing".into());
        }

        let probe = self.data_root.join(".maxio-readyz-probe");
        fs::write(&probe, b"1")
            .await
            .map_err(|e| format!("data directory not writable: {e}"))?;
        let _ = fs::remove_file(&probe).await;

        if !self.keyring.is_usable() {
            return Err("SSE-S3 keyring has no keys".into());
        }

        Ok(())
    }

    pub fn check_upload_start(&self, declared_size: Option<u64>) -> Result<(), StorageError> {
        self.quota.check_declared_size(declared_size)?;
        self.quota.check_disk_reserve(&self.data_root)
    }

    pub(super) fn wrap_upload_reader(&self, body: ByteStream) -> ByteStream {
        Box::pin(QuotaReader::new(body, self.quota, self.data_root.clone()))
    }

    // --- Bucket operations ---

    pub async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError> {
        validate_bucket_name(&meta.name)?;
        let bucket_dir = self.buckets_dir.join(&meta.name);
        match fs::create_dir(&bucket_dir).await {
            Ok(()) => {
                let meta_path = bucket_dir.join(".bucket.json");
                let json = serde_json::to_string_pretty(meta)?;
                if let Err(e) = fs::write(&meta_path, json).await {
                    // Clean up the empty directory to avoid a half-created bucket
                    let _ = fs::remove_dir(&bucket_dir).await;
                    return Err(e.into());
                }
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn head_bucket(&self, name: &str) -> Result<bool, StorageError> {
        validate_bucket_name(name)?;
        Ok(fs::try_exists(self.buckets_dir.join(name).join(".bucket.json")).await?)
    }

    pub async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError> {
        validate_bucket_name(name)?;
        let bucket_dir = self.buckets_dir.join(name);
        if !fs::try_exists(&bucket_dir).await? {
            return Ok(false);
        }
        // Pass 1: read-only walk. Any real object (data file, `.folder`
        // marker, `.ec/` chunk dir) at any depth → BucketNotEmpty.
        if self.has_real_objects(&bucket_dir).await? {
            return Err(StorageError::BucketNotEmpty);
        }
        // Pass 2: purge sidecars (`*.meta.json`), internal dirs
        // (`.uploads`, `.versions`), and empty subdirs at any depth.
        self.purge_empty_bucket(&bucket_dir).await?;
        match fs::remove_dir(&bucket_dir).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                // Concurrent writer slipped a file in between passes.
                Err(StorageError::BucketNotEmpty)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Read-only check: does this bucket contain any real object data?
    pub(super) fn has_real_objects<'a>(
        &'a self,
        dir: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut entries = fs::read_dir(dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == ".bucket.json"
                    || name == ".uploads"
                    || name == ".versions"
                    || name.ends_with(".meta.json")
                {
                    continue;
                }
                let ft = entry.file_type().await?;
                if ft.is_dir() && name.ends_with(".ec") {
                    return Ok(true);
                }
                if ft.is_dir() {
                    if self.has_real_objects(&entry.path()).await? {
                        return Ok(true);
                    }
                } else {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }

    /// Post-order purge: remove sidecars / internal dirs at any depth and
    /// empty out every subdirectory. Must only run after
    /// `has_real_objects` returned `false`.
    pub(super) fn purge_empty_bucket<'a>(
        &'a self,
        dir: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut entries = fs::read_dir(dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();
                let ft = entry.file_type().await?;

                if name == ".bucket.json"
                    || name == ".uploads"
                    || name == ".versions"
                    || name.ends_with(".meta.json")
                {
                    if ft.is_dir() {
                        fs::remove_dir_all(&path).await?;
                    } else {
                        fs::remove_file(&path).await?;
                    }
                    continue;
                }

                if ft.is_dir() {
                    self.purge_empty_bucket(&path).await?;
                    fs::remove_dir(&path).await?;
                }
                // Non-sidecar regular files shouldn't exist here
                // (has_real_objects would have rejected) — skip defensively.
            }
            Ok(())
        })
    }

    pub(super) fn object_path(&self, bucket: &str, key: &str) -> PathBuf {
        if key.ends_with('/') {
            let dir = key.trim_end_matches('/');
            self.buckets_dir.join(bucket).join(dir).join(".folder")
        } else {
            self.buckets_dir.join(bucket).join(key)
        }
    }

    pub(super) fn meta_path(&self, bucket: &str, key: &str) -> PathBuf {
        if key.ends_with('/') {
            let dir = key.trim_end_matches('/');
            self.buckets_dir
                .join(bucket)
                .join(dir)
                .join(".folder.meta.json")
        } else {
            self.buckets_dir
                .join(bucket)
                .join(format!("{}.meta.json", key))
        }
    }

    pub(super) fn ec_dir(&self, bucket: &str, key: &str) -> PathBuf {
        self.buckets_dir.join(bucket).join(format!("{}.ec", key))
    }

    pub(super) fn manifest_path(&self, bucket: &str, key: &str) -> PathBuf {
        self.ec_dir(bucket, key).join("manifest.json")
    }

    pub(super) async fn is_chunked_path(ec_dir: &Path) -> bool {
        matches!(fs::metadata(ec_dir).await, Ok(m) if m.is_dir())
    }

    pub(super) async fn read_manifest(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ChunkManifest, StorageError> {
        let path = self.manifest_path(bucket, key);
        let data = fs::read_to_string(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    pub(super) fn uploads_dir(&self, bucket: &str) -> PathBuf {
        self.buckets_dir.join(bucket).join(".uploads")
    }

    pub(super) fn upload_dir(&self, bucket: &str, upload_id: &str) -> PathBuf {
        self.uploads_dir(bucket).join(upload_id)
    }

    pub(super) fn upload_meta_path(&self, bucket: &str, upload_id: &str) -> PathBuf {
        self.upload_dir(bucket, upload_id).join(".meta.json")
    }

    pub(super) fn part_path(&self, bucket: &str, upload_id: &str, part_number: u32) -> PathBuf {
        self.upload_dir(bucket, upload_id)
            .join(part_number.to_string())
    }

    pub(super) fn part_meta_path(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
    ) -> PathBuf {
        self.upload_dir(bucket, upload_id)
            .join(format!("{}.meta.json", part_number))
    }

    pub(super) fn generate_version_id() -> String {
        let micros = chrono::Utc::now().timestamp_micros() as u64;
        let rand_suffix: u32 = rand::rng().random();
        format!("{:016}-{:08x}", micros, rand_suffix)
    }

    /// Directory holding versions for a given key.
    /// For key `photos/vacation.jpg` → `{bucket}/photos/.versions/vacation.jpg/`
    pub(super) fn versions_dir(&self, bucket: &str, key: &str) -> PathBuf {
        let key_path = Path::new(key);
        let parent = key_path.parent().unwrap_or(Path::new(""));
        let name = key_path.file_name().unwrap_or(std::ffi::OsStr::new(key));
        self.buckets_dir
            .join(bucket)
            .join(parent)
            .join(".versions")
            .join(name)
    }

    pub(super) fn version_data_path(&self, bucket: &str, key: &str, version_id: &str) -> PathBuf {
        self.versions_dir(bucket, key)
            .join(format!("{}.data", version_id))
    }

    pub(super) fn version_meta_path(&self, bucket: &str, key: &str, version_id: &str) -> PathBuf {
        self.versions_dir(bucket, key)
            .join(format!("{}.meta.json", version_id))
    }

    pub async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: BucketMeta = serde_json::from_str(&data)?;
        Ok(meta.versioning)
    }

    pub async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        let was_enabled = meta.versioning;
        meta.versioning = enabled;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;

        // S3-compatible suspension preserves historical versions. It only
        // changes how future writes/deletes are versioned.
        let _ = was_enabled;
        Ok(())
    }

    pub async fn get_bucket_public(&self, bucket: &str) -> Result<(bool, bool), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: BucketMeta = serde_json::from_str(&data)?;
        Ok((meta.public_read, meta.public_list))
    }

    pub async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let effects = match crate::policy::evaluate_v1_policy(bucket, policy) {
            Ok(effects) => effects,
            Err(_) => {
                crate::policy::validate_policy_v2(bucket, policy)
                    .map_err(StorageError::InvalidKey)?;
                crate::policy::PolicyEffects::default()
            }
        };
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.bucket_policy = Some(policy.to_string());
        meta.public_read = effects.public_read;
        meta.public_list = effects.public_list;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: BucketMeta = serde_json::from_str(&data)?;
        Ok(meta.bucket_policy)
    }

    pub async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.bucket_policy = None;
        meta.public_read = false;
        meta.public_list = false;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn set_bucket_public(
        &self,
        bucket: &str,
        read: bool,
        list: bool,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.public_read = read;
        meta.public_list = list;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn put_bucket_cors(
        &self,
        bucket: &str,
        rules: Vec<crate::CorsRule>,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.cors_rules = Some(rules);
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn get_bucket_cors(
        &self,
        bucket: &str,
    ) -> Result<Option<Vec<crate::CorsRule>>, StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: BucketMeta = serde_json::from_str(&data)?;
        Ok(meta.cors_rules)
    }

    pub async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.cors_rules = None;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    // --- Bucket default encryption ---

    pub async fn put_bucket_encryption(
        &self,
        bucket: &str,
        config: BucketEncryptionConfig,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.encryption_config = Some(config);
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn get_bucket_encryption(
        &self,
        bucket: &str,
    ) -> Result<Option<BucketEncryptionConfig>, StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: BucketMeta = serde_json::from_str(&data)?;
        Ok(meta.encryption_config)
    }

    pub async fn delete_bucket_encryption(&self, bucket: &str) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(bucket.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let mut meta: BucketMeta = serde_json::from_str(&data)?;
        meta.encryption_config = None;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }
}
