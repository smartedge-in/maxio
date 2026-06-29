//! Server-tier storage wrapper: bucket metadata mutations via storage Raft (P1-17/P1-20).

use async_trait::async_trait;
use maxio_storage::backend::{DynStorage, StorageBackend};
use maxio_storage::raft::StorageMutation;
use maxio_storage::{
    BucketMeta, ByteStream, ChecksumAlgorithm, CorsRule, DeleteResult, EncryptionRequest,
    LifecycleRule, MultipartUploadMeta, ObjectMeta, PartMeta, PutResult, StorageError,
    UploadEncryptionSpec,
};
use std::path::Path;
use std::sync::Arc;

use crate::client::StorageRaftClient;

/// Wraps local filesystem storage: bucket create/delete go through storage Raft; reads mirror locally.
///
/// **Phase 1 limitation:** object I/O (PUT/GET/multipart) still uses the server's local
/// [`FilesystemStorage`](maxio_storage::filesystem::FilesystemStorage). Only bucket metadata
/// mutations are replicated via Raft.
pub struct ClusterMetadataStorage {
    inner: DynStorage,
    raft: StorageRaftClient,
}

impl ClusterMetadataStorage {
    pub fn new(inner: DynStorage, raft: StorageRaftClient) -> Self {
        Self { inner, raft }
    }

    async fn propose_mutation(&self, mutation: StorageMutation) -> Result<(), StorageError> {
        let resp = self
            .raft
            .propose(mutation)
            .await
            .map_err(|e| StorageError::IntegrityError(format!("storage raft propose: {e}")))?;
        if resp.ok {
            Ok(())
        } else {
            Err(StorageError::IntegrityError(
                "storage raft propose rejected".into(),
            ))
        }
    }
}

/// Wrap an existing backend for cluster-mode bucket metadata routing.
pub fn wrap_cluster_storage(inner: DynStorage, raft: StorageRaftClient) -> DynStorage {
    Arc::new(ClusterMetadataStorage::new(inner, raft))
}

#[async_trait]
impl StorageBackend for ClusterMetadataStorage {
    fn data_root(&self) -> &Path {
        self.inner.data_root()
    }

    fn keyring(&self) -> &Arc<maxio_storage::keys::Keyring> {
        self.inner.keyring()
    }

    fn check_upload_start(&self, declared_size: Option<u64>) -> Result<(), StorageError> {
        self.inner.check_upload_start(declared_size)
    }

    async fn check_readiness(&self) -> Result<(), String> {
        self.inner.check_readiness().await
    }

    async fn housekeeping_sweep(&self, stale_after: chrono::Duration) -> (u64, u64, u64) {
        self.inner.housekeeping_sweep(stale_after).await
    }

    async fn count_active_multipart_uploads(&self) -> u64 {
        self.inner.count_active_multipart_uploads().await
    }

    async fn count_bucket_objects(&self, bucket: &str) -> Result<u64, StorageError> {
        self.inner.count_bucket_objects(bucket).await
    }

    async fn count_all_objects(&self) -> Result<u64, StorageError> {
        self.inner.count_all_objects().await
    }

    async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError> {
        self.inner.list_buckets().await
    }

    async fn create_bucket(&self, meta: &BucketMeta) -> Result<bool, StorageError> {
        self.propose_mutation(StorageMutation::CreateBucket {
            name: meta.name.clone(),
            region: meta.region.clone(),
        })
        .await?;
        self.inner.create_bucket(meta).await
    }

    async fn head_bucket(&self, name: &str) -> Result<bool, StorageError> {
        self.inner.head_bucket(name).await
    }

    async fn delete_bucket(&self, name: &str) -> Result<bool, StorageError> {
        self.propose_mutation(StorageMutation::DeleteBucket {
            name: name.to_string(),
        })
        .await?;
        self.inner.delete_bucket(name).await
    }

    async fn is_versioned(&self, bucket: &str) -> Result<bool, StorageError> {
        self.inner.is_versioned(bucket).await
    }

    async fn set_versioning(&self, bucket: &str, enabled: bool) -> Result<(), StorageError> {
        self.inner.set_versioning(bucket, enabled).await
    }

    async fn get_bucket_public(&self, bucket: &str) -> Result<(bool, bool), StorageError> {
        self.inner.get_bucket_public(bucket).await
    }

    async fn set_bucket_public(
        &self,
        bucket: &str,
        public_read: bool,
        public_list: bool,
    ) -> Result<(), StorageError> {
        self.inner
            .set_bucket_public(bucket, public_read, public_list)
            .await
    }

    async fn put_bucket_policy(&self, bucket: &str, policy: &str) -> Result<(), StorageError> {
        self.inner.put_bucket_policy(bucket, policy).await
    }

    async fn get_bucket_policy(&self, bucket: &str) -> Result<Option<String>, StorageError> {
        self.inner.get_bucket_policy(bucket).await
    }

    async fn delete_bucket_policy(&self, bucket: &str) -> Result<(), StorageError> {
        self.inner.delete_bucket_policy(bucket).await
    }

    async fn put_bucket_cors(
        &self,
        bucket: &str,
        rules: Vec<CorsRule>,
    ) -> Result<(), StorageError> {
        self.inner.put_bucket_cors(bucket, rules).await
    }

    async fn get_bucket_cors(&self, bucket: &str) -> Result<Option<Vec<CorsRule>>, StorageError> {
        self.inner.get_bucket_cors(bucket).await
    }

    async fn delete_bucket_cors(&self, bucket: &str) -> Result<(), StorageError> {
        self.inner.delete_bucket_cors(bucket).await
    }

    async fn put_bucket_encryption(
        &self,
        bucket: &str,
        config: maxio_storage::BucketEncryptionConfig,
    ) -> Result<(), StorageError> {
        self.inner.put_bucket_encryption(bucket, config).await
    }

    async fn get_bucket_encryption(
        &self,
        bucket: &str,
    ) -> Result<Option<maxio_storage::BucketEncryptionConfig>, StorageError> {
        self.inner.get_bucket_encryption(bucket).await
    }

    async fn delete_bucket_encryption(&self, bucket: &str) -> Result<(), StorageError> {
        self.inner.delete_bucket_encryption(bucket).await
    }

    async fn set_bucket_erasure_coding(
        &self,
        bucket: &str,
        enabled: Option<bool>,
    ) -> Result<(), StorageError> {
        self.inner.set_bucket_erasure_coding(bucket, enabled).await
    }

    async fn get_bucket_erasure_coding(&self, bucket: &str) -> Result<bool, StorageError> {
        self.inner.get_bucket_erasure_coding(bucket).await
    }

    async fn put_bucket_lifecycle(
        &self,
        bucket: &str,
        rules: Vec<LifecycleRule>,
    ) -> Result<(), StorageError> {
        self.inner.put_bucket_lifecycle(bucket, rules).await
    }

    async fn get_bucket_lifecycle(
        &self,
        bucket: &str,
    ) -> Result<Option<Vec<LifecycleRule>>, StorageError> {
        self.inner.get_bucket_lifecycle(bucket).await
    }

    async fn delete_bucket_lifecycle(&self, bucket: &str) -> Result<(), StorageError> {
        self.inner.delete_bucket_lifecycle(bucket).await
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
        self.inner
            .put_object(
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
        self.inner.get_object(bucket, key, customer_key).await
    }

    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        length: u64,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        self.inner
            .get_object_range(bucket, key, offset, length, customer_key)
            .await
    }

    async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        self.inner.head_object(bucket, key).await
    }

    async fn delete_object(&self, bucket: &str, key: &str) -> Result<DeleteResult, StorageError> {
        self.inner.delete_object(bucket, key).await
    }

    async fn get_object_tagging(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<std::collections::HashMap<String, String>, StorageError> {
        self.inner.get_object_tagging(bucket, key).await
    }

    async fn put_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tags: std::collections::HashMap<String, String>,
    ) -> Result<(), StorageError> {
        self.inner.put_object_tagging(bucket, key, tags).await
    }

    async fn delete_object_tagging(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        self.inner.delete_object_tagging(bucket, key).await
    }

    async fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        self.inner
            .get_object_version(bucket, key, version_id, customer_key)
            .await
    }

    async fn head_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        self.inner.head_object_version(bucket, key, version_id).await
    }

    async fn delete_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        self.inner
            .delete_object_version(bucket, key, version_id)
            .await
    }

    async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        self.inner.list_objects(bucket, prefix).await
    }

    async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        self.inner.list_object_versions(bucket, prefix).await
    }

    async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        checksum_algorithm: Option<ChecksumAlgorithm>,
        encryption_spec: Option<UploadEncryptionSpec>,
    ) -> Result<MultipartUploadMeta, StorageError> {
        self.inner
            .create_multipart_upload(bucket, key, content_type, checksum_algorithm, encryption_spec)
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
        self.inner
            .upload_part(
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
        self.inner
            .complete_multipart_upload(bucket, upload_id, parts, customer_key)
            .await
    }

    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError> {
        self.inner.abort_multipart_upload(bucket, upload_id).await
    }

    async fn list_parts(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(MultipartUploadMeta, Vec<PartMeta>), StorageError> {
        self.inner.list_parts(bucket, upload_id).await
    }

    async fn list_multipart_uploads(
        &self,
        bucket: &str,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError> {
        self.inner.list_multipart_uploads(bucket).await
    }
}