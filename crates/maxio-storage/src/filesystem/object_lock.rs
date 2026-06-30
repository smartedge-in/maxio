use super::FilesystemStorage;
use super::common::validate_key;
use crate::{
    LegalHoldStatus, ObjectLockConfig, ObjectLockRetention, ObjectMeta, StorageError,
    is_object_protected, validate_bucket_name,
};
use tokio::fs;

impl FilesystemStorage {
    pub async fn put_bucket_object_lock(
        &self,
        bucket: &str,
        config: ObjectLockConfig,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let mut meta = self.read_bucket_meta(bucket).await?;
        if !meta.object_lock_enabled {
            return Err(StorageError::InvalidKey(
                "object lock must be enabled at bucket creation".into(),
            ));
        }
        meta.object_lock_config = Some(config);
        fs::write(
            self.buckets_dir.join(bucket).join(".bucket.json"),
            serde_json::to_string_pretty(&meta)?,
        )
        .await?;
        Ok(())
    }

    pub async fn get_bucket_object_lock(
        &self,
        bucket: &str,
    ) -> Result<Option<ObjectLockConfig>, StorageError> {
        let meta = self.read_bucket_meta(bucket).await?;
        if !meta.object_lock_enabled {
            return Ok(None);
        }
        Ok(meta.object_lock_config)
    }

    pub(super) async fn ensure_can_overwrite(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<(), StorageError> {
        let meta_path = self.meta_path(bucket, key);
        if !fs::try_exists(&meta_path).await.unwrap_or(false) {
            return Ok(());
        }
        let meta = self.read_object_meta(bucket, key).await?;
        if is_object_protected(&meta) {
            return Err(StorageError::ObjectLocked(
                "object is protected by retention or legal hold".into(),
            ));
        }
        Ok(())
    }

    pub(super) async fn resolve_default_object_lock(
        &self,
        bucket: &str,
    ) -> Result<Option<ObjectLockRetention>, StorageError> {
        let meta = self.read_bucket_meta(bucket).await?;
        if !meta.object_lock_enabled {
            return Ok(None);
        }
        let Some(config) = meta.object_lock_config.as_ref() else {
            return Ok(None);
        };
        let (Some(mode), Some(days)) =
            (config.default_retention_mode, config.default_retention_days)
        else {
            return Ok(None);
        };
        if days == 0 {
            return Ok(None);
        }
        let until = chrono::Utc::now() + chrono::Duration::days(days as i64);
        Ok(Some(ObjectLockRetention {
            mode,
            retain_until_date: until.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        }))
    }

    pub(super) fn apply_object_lock(meta: &mut ObjectMeta, lock: Option<ObjectLockRetention>) {
        if let Some(lock) = lock {
            meta.object_lock_mode = Some(lock.mode);
            meta.retain_until_date = Some(lock.retain_until_date);
        }
    }

    async fn read_meta_at(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<ObjectMeta, StorageError> {
        validate_key(key)?;
        if let Some(vid) = version_id {
            if vid == "null" {
                return self.read_object_meta(bucket, key).await;
            }
            let path = self.version_meta_path(bucket, key, vid);
            let data = fs::read_to_string(&path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::VersionNotFound(vid.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            return Ok(serde_json::from_str(&data)?);
        }
        self.read_object_meta(bucket, key).await
    }

    async fn write_meta_at(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
        meta: &ObjectMeta,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string_pretty(meta)?;
        match version_id {
            Some(vid) if vid == "null" => fs::write(self.meta_path(bucket, key), json).await?,
            Some(vid) => fs::write(self.version_meta_path(bucket, key, vid), json).await?,
            None => fs::write(self.meta_path(bucket, key), json).await?,
        }
        Ok(())
    }

    pub async fn put_object_retention(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
        retention: ObjectLockRetention,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        if !self.read_bucket_meta(bucket).await?.object_lock_enabled {
            return Err(StorageError::InvalidKey(
                "object lock is not enabled for this bucket".into(),
            ));
        }
        let mut meta = self.read_meta_at(bucket, key, version_id).await?;
        meta.object_lock_mode = Some(retention.mode);
        meta.retain_until_date = Some(retention.retain_until_date);
        self.write_meta_at(bucket, key, version_id, &meta).await
    }

    pub async fn get_object_retention(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<ObjectLockRetention, StorageError> {
        let meta = self.read_meta_at(bucket, key, version_id).await?;
        match (meta.object_lock_mode, meta.retain_until_date) {
            (Some(mode), Some(retain_until_date)) => Ok(ObjectLockRetention {
                mode,
                retain_until_date,
            }),
            _ => Err(StorageError::NotFound(key.to_string())),
        }
    }

    pub async fn put_object_legal_hold(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
        status: LegalHoldStatus,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        if !self.read_bucket_meta(bucket).await?.object_lock_enabled {
            return Err(StorageError::InvalidKey(
                "object lock is not enabled for this bucket".into(),
            ));
        }
        let mut meta = self.read_meta_at(bucket, key, version_id).await?;
        meta.legal_hold_status = Some(status);
        self.write_meta_at(bucket, key, version_id, &meta).await
    }

    pub async fn get_object_legal_hold(
        &self,
        bucket: &str,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<LegalHoldStatus, StorageError> {
        let meta = self.read_meta_at(bucket, key, version_id).await?;
        meta.legal_hold_status
            .ok_or_else(|| StorageError::NotFound(key.to_string()))
    }

    pub(super) async fn ensure_can_delete_meta(
        &self,
        meta: &ObjectMeta,
    ) -> Result<(), StorageError> {
        if is_object_protected(meta) {
            return Err(StorageError::ObjectLocked(
                "object is protected by retention or legal hold".into(),
            ));
        }
        Ok(())
    }
}
