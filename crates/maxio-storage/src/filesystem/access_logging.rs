use super::*;

/// S3 server access log line (tab-separated subset).
#[derive(Debug, Clone)]
pub struct AccessLogEntry {
    pub timestamp: String,
    pub source_bucket: String,
    pub remote_ip: String,
    pub requester: String,
    pub request_id: String,
    pub operation: String,
    pub key: String,
    pub request_uri: String,
    pub http_status: u16,
    pub error_code: String,
    pub bytes_sent: u64,
    pub object_size: u64,
    pub total_time_ms: u64,
    pub user_agent: String,
}

impl AccessLogEntry {
    pub fn format_line(&self) -> String {
        format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
            self.source_bucket,
            self.timestamp,
            self.remote_ip,
            self.requester,
            self.request_id,
            self.operation,
            self.key,
            self.request_uri,
            self.http_status,
            self.error_code,
            self.bytes_sent,
            self.object_size,
            self.total_time_ms,
            self.user_agent,
        )
    }
}

impl FilesystemStorage {
    pub async fn get_bucket_logging(
        &self,
        bucket: &str,
    ) -> Result<Option<(String, String)>, StorageError> {
        let meta = self.read_bucket_meta(bucket).await?;
        match meta.logging_target_bucket {
            Some(target) => Ok(Some((
                target,
                meta.logging_target_prefix.unwrap_or_default(),
            ))),
            None => Ok(None),
        }
    }

    pub async fn put_bucket_logging(
        &self,
        bucket: &str,
        target_bucket: &str,
        target_prefix: &str,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        validate_bucket_name(target_bucket)?;
        if bucket == target_bucket {
            return Err(StorageError::InvalidKey(
                "logging target bucket must differ from source bucket".into(),
            ));
        }
        if !self.head_bucket(target_bucket).await? {
            return Err(StorageError::NotFound(target_bucket.to_string()));
        }
        let mut meta = self.read_bucket_meta(bucket).await?;
        meta.logging_target_bucket = Some(target_bucket.to_string());
        meta.logging_target_prefix = if target_prefix.is_empty() {
            None
        } else {
            Some(target_prefix.to_string())
        };
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn delete_bucket_logging(&self, bucket: &str) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let mut meta = self.read_bucket_meta(bucket).await?;
        meta.logging_target_bucket = None;
        meta.logging_target_prefix = None;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn deliver_access_log(&self, entry: &AccessLogEntry) -> Result<(), StorageError> {
        let meta = self.read_bucket_meta(&entry.source_bucket).await?;
        let (target_bucket, prefix) = match meta.logging_target_bucket {
            Some(ref t) => (t.clone(), meta.logging_target_prefix.unwrap_or_default()),
            None => return Ok(()),
        };
        if target_bucket == entry.source_bucket {
            return Ok(());
        }

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let log_key = if prefix.is_empty() {
            format!("{}/{}.log", entry.source_bucket, date)
        } else {
            let p = if prefix.ends_with('/') {
                prefix.clone()
            } else {
                format!("{prefix}/")
            };
            format!("{p}{}/{}.log", entry.source_bucket, date)
        };

        let line = entry.format_line();
        let obj_path = self.object_path(&target_bucket, &log_key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut existing = match fs::read(&obj_path).await {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(StorageError::Io(e)),
        };
        existing.extend_from_slice(line.as_bytes());
        fs::write(&obj_path, &existing).await?;

        let meta_path = self.meta_path(&target_bucket, &log_key);
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let etag = format!("\"{:x}\"", md5::Md5::digest(&existing));
        let object_meta = ObjectMeta {
            key: log_key,
            size: existing.len() as u64,
            etag,
            content_type: "text/plain".to_string(),
            last_modified: now,
            version_id: None,
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm: None,
            checksum_value: None,
            tags: None,
            part_sizes: None,
            encryption: None,
            object_lock_mode: None,
            retain_until_date: None,
            legal_hold_status: None,
        };
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&meta_path, serde_json::to_string_pretty(&object_meta)?).await?;
        Ok(())
    }

    pub async fn get_bucket_notification(
        &self,
        bucket: &str,
    ) -> Result<Option<BucketNotificationConfig>, StorageError> {
        Ok(self.read_bucket_meta(bucket).await?.notification_config)
    }

    pub async fn put_bucket_notification(
        &self,
        bucket: &str,
        config: BucketNotificationConfig,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let mut meta = self.read_bucket_meta(bucket).await?;
        meta.notification_config = Some(config);
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }

    pub async fn delete_bucket_notification(&self, bucket: &str) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        let mut meta = self.read_bucket_meta(bucket).await?;
        meta.notification_config = None;
        let meta_path = self.buckets_dir.join(bucket).join(".bucket.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        Ok(())
    }
}
