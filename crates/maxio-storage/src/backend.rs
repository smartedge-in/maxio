//! Object storage backend abstraction (P1-15).
//!
//! All S3 metadata and object mutations go through [`StorageBackend`] so a future
//! Raft-backed implementation can replace [`FilesystemStorage`] without touching
//! HTTP handlers.

use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

use crate::filesystem::AccessLogEntry;
use crate::filesystem::FilesystemStorage;
use crate::keys::Keyring;
use crate::kms::KmsBackend;
use crate::{
    BucketMeta, BucketNotificationConfig, ByteStream, ChecksumAlgorithm, CorsRule, DeleteResult,
    EncryptionRequest, LegalHoldStatus, LifecycleRule, MultipartUploadMeta, ObjectLockConfig,
    ObjectLockRetention, ObjectMeta, PartMeta, PutResult, StorageError, UploadEncryptionSpec,
};

/// Shared handle to a storage backend (filesystem today; Raft in P1-17+).
pub type DynStorage = Arc<dyn StorageBackend>;

/// Wrap a concrete [`FilesystemStorage`] for use in the server tier.
pub fn dyn_storage(storage: FilesystemStorage) -> DynStorage {
    Arc::new(storage)
}

#[allow(clippy::too_many_arguments)]
#[async_trait]
pub trait StorageBackend: Send + Sync {
    // --- Operational ---

    fn data_root(&self) -> &Path;
    fn keyring(&self) -> &Arc<Keyring>;
    fn kms(&self) -> Option<&Arc<dyn KmsBackend>>;
    fn check_upload_start(&self, declared_size: Option<u64>) -> Result<(), StorageError>;

    async fn check_readiness(&self) -> Result<(), String>;
    async fn housekeeping_sweep(&self, stale_after: chrono::Duration) -> (u64, u64, u64);
    async fn count_active_multipart_uploads(&self) -> u64;
    async fn count_bucket_objects(&self, bucket: &str) -> Result<u64, StorageError>;
    async fn count_all_objects(&self) -> Result<u64, StorageError>;

    // --- Buckets ---

    async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError>;
    async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError>;
    async fn head_bucket(&self, name: &str) -> Result<bool, StorageError>;
    async fn get_bucket_meta(&self, bucket: &str) -> Result<BucketMeta, StorageError>;
    async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError>;
    async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError>;
    async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError>;
    async fn get_bucket_public(&self, bucket: &str) -> Result<(bool, bool), StorageError>;
    async fn set_bucket_public(
        &self,
        bucket: &str,
        public_read: bool,
        public_list: bool,
    ) -> Result<(), StorageError>;
    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError>;
    async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError>;
    async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError>;
    async fn put_bucket_cors(&self, bucket: &str, rules: Vec<CorsRule>)
    -> Result<(), StorageError>;
    async fn get_bucket_cors(&self, bucket: &str) -> Result<Option<Vec<CorsRule>>, StorageError>;
    async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError>;
    async fn put_bucket_encryption(
        &self,
        bucket: &str,
        config: crate::BucketEncryptionConfig,
    ) -> Result<(), StorageError>;
    async fn get_bucket_encryption(
        &self,
        bucket: &str,
    ) -> Result<Option<crate::BucketEncryptionConfig>, StorageError>;
    async fn delete_bucket_encryption(&self, bucket: &str) -> Result<(), StorageError>;
    async fn set_bucket_erasure_coding(
        &self,
        bucket: &str,
        enabled: Option<bool>,
    ) -> Result<(), StorageError>;
    async fn get_bucket_erasure_coding(&self, bucket: &str) -> Result<bool, StorageError>;
    async fn put_bucket_lifecycle(
        &self,
        bucket: &str,
        rules: Vec<LifecycleRule>,
    ) -> Result<(), StorageError>;
    async fn get_bucket_lifecycle(
        &self,
        bucket: &str,
    ) -> Result<Option<Vec<LifecycleRule>>, StorageError>;
    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<(), StorageError>;
    async fn get_bucket_logging(
        &self,
        bucket: &str,
    ) -> Result<Option<(String, String)>, StorageError>;
    async fn put_bucket_logging(
        &self,
        bucket: &str,
        target_bucket: &str,
        target_prefix: &str,
    ) -> Result<(), StorageError>;
    async fn delete_bucket_logging(&self, bucket: &str) -> Result<(), StorageError>;
    async fn get_bucket_notification(
        &self,
        bucket: &str,
    ) -> Result<Option<BucketNotificationConfig>, StorageError>;
    async fn put_bucket_notification(
        &self,
        bucket: &str,
        config: BucketNotificationConfig,
    ) -> Result<(), StorageError>;
    async fn delete_bucket_notification(&self, bucket: &str) -> Result<(), StorageError>;
    async fn deliver_access_log(&self, entry: &AccessLogEntry) -> Result<(), StorageError>;
    async fn put_bucket_object_lock(
        &self,
        bucket: &str,
        config: ObjectLockConfig,
    ) -> Result<(), StorageError>;
    async fn get_bucket_object_lock(
        &self,
        bucket: &str,
    ) -> Result<Option<ObjectLockConfig>, StorageError>;
    async fn put_object_retention(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
        retention: ObjectLockRetention,
    ) -> Result<(), StorageError>;
    async fn get_object_retention(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<ObjectLockRetention, StorageError>;
    async fn put_object_legal_hold(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
        status: LegalHoldStatus,
    ) -> Result<(), StorageError>;
    async fn get_object_legal_hold(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<LegalHoldStatus, StorageError>;

    // --- Objects ---

    async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
        encryption: Option<EncryptionRequest>,
        declared_size: Option<u64>,
    ) -> Result<PutResult, StorageError>;

    async fn get_object(
        &self,
        bucket: &str,
        key: &str,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError>;

    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        length: u64,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError>;

    async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError>;
    async fn delete_object(&self, bucket: &str, key: &str) -> Result<DeleteResult, StorageError>;
    async fn get_object_tagging(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<std::collections::HashMap<String, String>, StorageError>;
    async fn put_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tags: std::collections::HashMap<String, String>,
    ) -> Result<(), StorageError>;
    async fn delete_object_tagging(&self, bucket: &str, key: &str) -> Result<(), StorageError>;
    async fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError>;
    async fn head_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError>;
    async fn delete_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError>;

    // --- Listing ---

    async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError>;
    async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError>;

    // --- Multipart ---

    async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        checksum_algorithm: Option<ChecksumAlgorithm>,
        encryption_spec: Option<UploadEncryptionSpec>,
    ) -> Result<MultipartUploadMeta, StorageError>;

    async fn upload_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
        customer_key: Option<[u8; 32]>,
        declared_size: Option<u64>,
    ) -> Result<PartMeta, StorageError>;

    async fn complete_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
        parts: &[(u32, String)],
        customer_key: Option<[u8; 32]>,
    ) -> Result<PutResult, StorageError>;

    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError>;
    async fn list_parts(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(MultipartUploadMeta, Vec<PartMeta>), StorageError>;
    async fn list_multipart_uploads(
        &self,
        bucket: &str,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError>;
}

#[async_trait]
impl StorageBackend for FilesystemStorage {
    fn data_root(&self) -> &Path {
        self.data_root()
    }

    fn keyring(&self) -> &Arc<Keyring> {
        self.keyring()
    }

    fn kms(&self) -> Option<&Arc<dyn KmsBackend>> {
        self.kms_backend()
    }

    fn check_upload_start(&self, declared_size: Option<u64>) -> Result<(), StorageError> {
        self.check_upload_start(declared_size)
    }

    async fn check_readiness(&self) -> Result<(), String> {
        self.check_readiness().await
    }

    async fn housekeeping_sweep(&self, stale_after: chrono::Duration) -> (u64, u64, u64) {
        self.housekeeping_sweep(stale_after).await
    }

    async fn count_active_multipart_uploads(&self) -> u64 {
        self.count_active_multipart_uploads().await
    }

    async fn count_bucket_objects(&self, bucket: &str) -> Result<u64, StorageError> {
        self.count_bucket_objects(bucket).await
    }

    async fn count_all_objects(&self) -> Result<u64, StorageError> {
        self.count_all_objects().await
    }

    async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError> {
        self.list_buckets().await
    }

    async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError> {
        self.create_bucket(meta).await
    }

    async fn head_bucket(&self, name: &str) -> Result<bool, StorageError> {
        self.head_bucket(name).await
    }

    async fn get_bucket_meta(&self, bucket: &str) -> Result<BucketMeta, StorageError> {
        self.get_bucket_meta(bucket).await
    }

    async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError> {
        self.delete_bucket(name).await
    }

    async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError> {
        self.is_versioned(bucket).await
    }

    async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError> {
        self.set_versioning(bucket, enabled).await
    }

    async fn get_bucket_public(&self, bucket: &str) -> Result<(bool, bool), StorageError> {
        self.get_bucket_public(bucket).await
    }

    async fn set_bucket_public(
        &self,
        bucket: &str,
        public_read: bool,
        public_list: bool,
    ) -> Result<(), StorageError> {
        self.set_bucket_public(bucket, public_read, public_list)
            .await
    }

    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError> {
        self.put_bucket_policy(bucket, policy).await
    }

    async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError> {
        self.get_bucket_policy(bucket).await
    }

    async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError> {
        self.delete_bucket_policy(bucket).await
    }

    async fn put_bucket_cors(
        &self,
        bucket: &str,
        rules: Vec<CorsRule>,
    ) -> Result<(), StorageError> {
        self.put_bucket_cors(bucket, rules).await
    }

    async fn get_bucket_cors(&self, bucket: &str) -> Result<Option<Vec<CorsRule>>, StorageError> {
        self.get_bucket_cors(bucket).await
    }

    async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError> {
        self.delete_bucket_cors(bucket).await
    }

    async fn put_bucket_encryption(
        &self,
        bucket: &str,
        config: crate::BucketEncryptionConfig,
    ) -> Result<(), StorageError> {
        self.put_bucket_encryption(bucket, config).await
    }

    async fn get_bucket_encryption(
        &self,
        bucket: &str,
    ) -> Result<Option<crate::BucketEncryptionConfig>, StorageError> {
        self.get_bucket_encryption(bucket).await
    }

    async fn delete_bucket_encryption(&self, bucket: &str) -> Result<(), StorageError> {
        self.delete_bucket_encryption(bucket).await
    }

    async fn set_bucket_erasure_coding(
        &self,
        bucket: &str,
        enabled: Option<bool>,
    ) -> Result<(), StorageError> {
        self.set_bucket_erasure_coding(bucket, enabled).await
    }

    async fn get_bucket_erasure_coding(&self, bucket: &str) -> Result<bool, StorageError> {
        self.get_bucket_erasure_coding(bucket).await
    }

    async fn put_bucket_lifecycle(
        &self,
        bucket: &str,
        rules: Vec<LifecycleRule>,
    ) -> Result<(), StorageError> {
        self.put_bucket_lifecycle(bucket, rules).await
    }

    async fn get_bucket_lifecycle(
        &self,
        bucket: &str,
    ) -> Result<Option<Vec<LifecycleRule>>, StorageError> {
        self.get_bucket_lifecycle(bucket).await
    }

    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<(), StorageError> {
        self.delete_bucket_lifecycle(bucket).await
    }

    async fn get_bucket_logging(
        &self,
        bucket: &str,
    ) -> Result<Option<(String, String)>, StorageError> {
        self.get_bucket_logging(bucket).await
    }

    async fn put_bucket_logging(
        &self,
        bucket: &str,
        target_bucket: &str,
        target_prefix: &str,
    ) -> Result<(), StorageError> {
        self.put_bucket_logging(bucket, target_bucket, target_prefix)
            .await
    }

    async fn delete_bucket_logging(&self, bucket: &str) -> Result<(), StorageError> {
        self.delete_bucket_logging(bucket).await
    }

    async fn get_bucket_notification(
        &self,
        bucket: &str,
    ) -> Result<Option<BucketNotificationConfig>, StorageError> {
        self.get_bucket_notification(bucket).await
    }

    async fn put_bucket_notification(
        &self,
        bucket: &str,
        config: BucketNotificationConfig,
    ) -> Result<(), StorageError> {
        self.put_bucket_notification(bucket, config).await
    }

    async fn delete_bucket_notification(&self, bucket: &str) -> Result<(), StorageError> {
        self.delete_bucket_notification(bucket).await
    }

    async fn deliver_access_log(&self, entry: &AccessLogEntry) -> Result<(), StorageError> {
        self.deliver_access_log(entry).await
    }

    async fn put_bucket_object_lock(
        &self,
        bucket: &str,
        config: ObjectLockConfig,
    ) -> Result<(), StorageError> {
        self.put_bucket_object_lock(bucket, config).await
    }

    async fn get_bucket_object_lock(
        &self,
        bucket: &str,
    ) -> Result<Option<ObjectLockConfig>, StorageError> {
        self.get_bucket_object_lock(bucket).await
    }

    async fn put_object_retention(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
        retention: ObjectLockRetention,
    ) -> Result<(), StorageError> {
        self.put_object_retention(bucket, key, version_id, retention)
            .await
    }

    async fn get_object_retention(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<ObjectLockRetention, StorageError> {
        self.get_object_retention(bucket, key, version_id).await
    }

    async fn put_object_legal_hold(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
        status: LegalHoldStatus,
    ) -> Result<(), StorageError> {
        self.put_object_legal_hold(bucket, key, version_id, status)
            .await
    }

    async fn get_object_legal_hold(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<LegalHoldStatus, StorageError> {
        self.get_object_legal_hold(bucket, key, version_id).await
    }

    async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
        encryption: Option<EncryptionRequest>,
        declared_size: Option<u64>,
    ) -> Result<PutResult, StorageError> {
        self.put_object(
            bucket,
            key,
            content_type,
            body,
            checksum,
            encryption,
            declared_size,
        )
        .await
    }

    async fn get_object(
        &self,
        bucket: &str,
        key: &str,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        self.get_object(bucket, key, customer_key).await
    }

    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        length: u64,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        self.get_object_range(bucket, key, offset, length, customer_key)
            .await
    }

    async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        self.head_object(bucket, key).await
    }

    async fn delete_object(&self, bucket: &str, key: &str) -> Result<DeleteResult, StorageError> {
        self.delete_object(bucket, key).await
    }

    async fn get_object_tagging(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<std::collections::HashMap<String, String>, StorageError> {
        self.get_object_tagging(bucket, key).await
    }

    async fn put_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tags: std::collections::HashMap<String, String>,
    ) -> Result<(), StorageError> {
        self.put_object_tagging(bucket, key, tags).await
    }

    async fn delete_object_tagging(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        self.delete_object_tagging(bucket, key).await
    }

    async fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        self.get_object_version(bucket, key, version_id, customer_key)
            .await
    }

    async fn head_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        self.head_object_version(bucket, key, version_id).await
    }

    async fn delete_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        self.delete_object_version(bucket, key, version_id).await
    }

    async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        self.list_objects(bucket, prefix).await
    }

    async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        self.list_object_versions(bucket, prefix).await
    }

    async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        checksum_algorithm: Option<ChecksumAlgorithm>,
        encryption_spec: Option<UploadEncryptionSpec>,
    ) -> Result<MultipartUploadMeta, StorageError> {
        self.create_multipart_upload(
            bucket,
            key,
            content_type,
            checksum_algorithm,
            encryption_spec,
        )
        .await
    }

    async fn upload_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
        customer_key: Option<[u8; 32]>,
        declared_size: Option<u64>,
    ) -> Result<PartMeta, StorageError> {
        self.upload_part(
            bucket,
            upload_id,
            part_number,
            body,
            checksum,
            customer_key,
            declared_size,
        )
        .await
    }

    async fn complete_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
        parts: &[(u32, String)],
        customer_key: Option<[u8; 32]>,
    ) -> Result<PutResult, StorageError> {
        self.complete_multipart_upload(bucket, upload_id, parts, customer_key)
            .await
    }

    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError> {
        self.abort_multipart_upload(bucket, upload_id).await
    }

    async fn list_parts(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(MultipartUploadMeta, Vec<PartMeta>), StorageError> {
        self.list_parts(bucket, upload_id).await
    }

    async fn list_multipart_uploads(
        &self,
        bucket: &str,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError> {
        self.list_multipart_uploads(bucket).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::QuotaLimits;
    use tempfile::tempdir;

    #[tokio::test]
    async fn dyn_storage_implements_trait() {
        let dir = tempdir().unwrap();
        let keyring = Arc::new(
            Keyring::load(dir.path().to_str().unwrap(), None)
                .await
                .unwrap(),
        );
        let fs = FilesystemStorage::new(
            dir.path().to_str().unwrap(),
            false,
            1024 * 1024,
            0,
            keyring,
            None,
            QuotaLimits::from_config(0, 0),
            false,
        )
        .await
        .unwrap();
        let backend: DynStorage = dyn_storage(fs);
        assert!(backend.check_readiness().await.is_ok());
        assert!(backend.list_buckets().await.unwrap().is_empty());
    }
}
