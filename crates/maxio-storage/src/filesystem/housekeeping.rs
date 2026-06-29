use super::*;

impl FilesystemStorage {
    pub async fn housekeeping_sweep(&self, stale_after: chrono::Duration) -> (u64, u64, u64) {
        let now = chrono::Utc::now();
        let mut uploads_removed = 0u64;
        let mut temp_removed = 0u64;
        let mut objects_expired = 0u64;

        let mut bucket_entries = match fs::read_dir(&self.buckets_dir).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("housekeeping: cannot read buckets dir: {}", e);
                return (0, 0, 0);
            }
        };

        while let Ok(Some(bucket_entry)) = bucket_entries.next_entry().await {
            if !matches!(bucket_entry.file_type().await, Ok(ft) if ft.is_dir()) {
                continue;
            }
            let bucket_name = bucket_entry.file_name().to_string_lossy().to_string();
            let bucket_dir = bucket_entry.path();

            // 1. Stale multipart uploads.
            let uploads_dir = bucket_dir.join(".uploads");
            if let Ok(mut uploads) = fs::read_dir(&uploads_dir).await {
                while let Ok(Some(up)) = uploads.next_entry().await {
                    if !matches!(up.file_type().await, Ok(ft) if ft.is_dir()) {
                        continue;
                    }
                    let meta_path = up.path().join(".meta.json");
                    let initiated = fs::read_to_string(&meta_path)
                        .await
                        .ok()
                        .and_then(|d| serde_json::from_str::<MultipartUploadMeta>(&d).ok())
                        .map(|m| m.initiated);
                    let age_ok = match initiated {
                        Some(ts) => chrono::DateTime::parse_from_rfc3339(&ts)
                            .map(|t| now.signed_duration_since(t.with_timezone(&chrono::Utc)))
                            .map(|age| age > stale_after)
                            .unwrap_or(true),
                        None => true,
                    };
                    if age_ok {
                        match fs::remove_dir_all(up.path()).await {
                            Ok(()) => {
                                uploads_removed += 1;
                                tracing::info!(
                                    "housekeeping: aborted stale multipart upload {}",
                                    up.file_name().to_string_lossy()
                                );
                            }
                            Err(e) => tracing::warn!(
                                "housekeeping: failed to remove stale upload {}: {}",
                                up.path().display(),
                                e
                            ),
                        }
                    }
                }
            }

            // 2. Leftover temp files from crashed writes.
            temp_removed += Self::sweep_temp_files(&bucket_dir).await;
            temp_removed += Self::sweep_temp_files(&uploads_dir).await;

            // 3. Lifecycle expiration (P3-01).
            objects_expired += self.sweep_lifecycle_expiration(&bucket_name, now).await;
        }

        (uploads_removed, temp_removed, objects_expired)
    }

    async fn sweep_lifecycle_expiration(
        &self,
        bucket: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> u64 {
        let rules = match self.read_bucket_meta(bucket).await {
            Ok(meta) => meta.lifecycle_rules.unwrap_or_default(),
            Err(_) => return 0,
        };
        let active_rules: Vec<_> = rules.into_iter().filter(|r| r.enabled).collect();
        if active_rules.is_empty() {
            return 0;
        }

        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        if versioned {
            tracing::debug!(
                "housekeeping: skipping lifecycle expiration for versioned bucket {bucket}"
            );
            return 0;
        }

        let objects = match self.list_objects(bucket, "").await {
            Ok(o) => o,
            Err(err) => {
                tracing::warn!("housekeeping: cannot list {bucket} for lifecycle: {err}");
                return 0;
            }
        };

        let mut removed = 0u64;
        for meta in objects {
            if meta.is_delete_marker {
                continue;
            }
            let Some(expiry_days) = Self::lifecycle_expiry_days_for_key(&active_rules, &meta.key)
            else {
                continue;
            };
            let Some(modified) = chrono::DateTime::parse_from_rfc3339(&meta.last_modified).ok()
            else {
                continue;
            };
            let age = now.signed_duration_since(modified.with_timezone(&chrono::Utc));
            if age.num_days() < expiry_days as i64 {
                continue;
            }
            match self.delete_object(bucket, &meta.key).await {
                Ok(_) => {
                    removed += 1;
                    tracing::info!(
                        "housekeeping: expired object {bucket}/{} (rule >= {expiry_days} days)",
                        meta.key
                    );
                }
                Err(err) => tracing::warn!(
                    "housekeeping: failed to expire {bucket}/{}: {err}",
                    meta.key
                ),
            }
        }
        removed
    }

    fn lifecycle_expiry_days_for_key(rules: &[LifecycleRule], key: &str) -> Option<u32> {
        rules
            .iter()
            .filter(|r| key.starts_with(&r.prefix))
            .max_by_key(|r| r.prefix.len())
            .map(|r| r.expiration_days)
    }

    /// Count in-progress multipart uploads across all buckets.
    pub async fn count_active_multipart_uploads(&self) -> u64 {
        let mut total = 0u64;
        let mut bucket_entries = match fs::read_dir(&self.buckets_dir).await {
            Ok(e) => e,
            Err(_) => return 0,
        };
        while let Ok(Some(bucket_entry)) = bucket_entries.next_entry().await {
            if !matches!(bucket_entry.file_type().await, Ok(ft) if ft.is_dir()) {
                continue;
            }
            let uploads_dir = bucket_entry.path().join(".uploads");
            let mut uploads = match fs::read_dir(&uploads_dir).await {
                Ok(e) => e,
                Err(_) => continue,
            };
            while let Ok(Some(up)) = uploads.next_entry().await {
                if matches!(up.file_type().await, Ok(ft) if ft.is_dir()) {
                    total += 1;
                }
            }
        }
        total
    }

    /// Remove `.maxio-tmp-*` entries directly inside `dir`.
    pub(super) async fn sweep_temp_files(dir: &Path) -> u64 {
        let mut removed = 0u64;
        let mut entries = match fs::read_dir(dir).await {
            Ok(e) => e,
            Err(_) => return 0,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(".maxio-tmp-") {
                continue;
            }
            let path = entry.path();
            let result = match entry.file_type().await {
                Ok(ft) if ft.is_dir() => fs::remove_dir_all(&path).await,
                _ => fs::remove_file(&path).await,
            };
            match result {
                Ok(()) => {
                    removed += 1;
                    tracing::info!("housekeeping: removed leftover temp {}", path.display());
                }
                Err(e) => {
                    tracing::warn!(
                        "housekeeping: failed to remove temp {}: {}",
                        path.display(),
                        e
                    )
                }
            }
        }
        removed
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::LifecycleRule;
    use crate::keys::Keyring;
    use crate::quota::QuotaLimits;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn lifecycle_expires_objects_matching_prefix() {
        let tmp = TempDir::new().unwrap();
        let keyring = Arc::new(
            Keyring::load(tmp.path().to_str().unwrap(), None)
                .await
                .unwrap(),
        );
        let storage = FilesystemStorage::new(
            tmp.path().to_str().unwrap(),
            false,
            1024,
            0,
            keyring,
            QuotaLimits::from_config(0, 0),
            false,
        )
        .await
        .unwrap();

        let bucket = "logs";
        storage
            .create_bucket(&BucketMeta {
                name: bucket.into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
                region: "us-east-1".into(),
                versioning: false,
                cors_rules: None,
                encryption_config: None,
                public_read: false,
                public_list: false,
                bucket_policy: None,
                erasure_coding: None,
                lifecycle_rules: None,
            })
            .await
            .unwrap();

        storage
            .put_bucket_lifecycle(
                bucket,
                vec![LifecycleRule {
                    id: "expire-logs".into(),
                    prefix: "old/".into(),
                    expiration_days: 1,
                    enabled: true,
                }],
            )
            .await
            .unwrap();

        let meta_path = storage.meta_path(bucket, "old/file.txt");
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await.unwrap();
        }
        let stale = ObjectMeta {
            key: "old/file.txt".into(),
            size: 3,
            etag: "\"x\"".into(),
            content_type: "text/plain".into(),
            last_modified: "2020-01-01T00:00:00.000Z".into(),
            version_id: None,
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm: None,
            checksum_value: None,
            tags: None,
            part_sizes: None,
            encryption: None,
        };
        fs::write(&meta_path, serde_json::to_string_pretty(&stale).unwrap())
            .await
            .unwrap();
        fs::write(storage.object_path(bucket, "old/file.txt"), b"abc")
            .await
            .unwrap();

        let removed = storage
            .sweep_lifecycle_expiration(bucket, chrono::Utc::now())
            .await;
        assert_eq!(removed, 1);
        assert!(!fs::try_exists(&meta_path).await.unwrap());
    }

    #[test]
    fn longest_prefix_rule_wins() {
        let rules = vec![
            LifecycleRule {
                id: "a".into(),
                prefix: "logs/".into(),
                expiration_days: 30,
                enabled: true,
            },
            LifecycleRule {
                id: "b".into(),
                prefix: "logs/2024/".into(),
                expiration_days: 7,
                enabled: true,
            },
        ];
        assert_eq!(
            FilesystemStorage::lifecycle_expiry_days_for_key(&rules, "logs/2024/jan.log"),
            Some(7)
        );
    }
}
