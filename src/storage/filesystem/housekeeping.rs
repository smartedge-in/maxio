use super::*;

impl FilesystemStorage {
    pub async fn housekeeping_sweep(&self, stale_after: chrono::Duration) -> (u64, u64) {
        let now = chrono::Utc::now();
        let mut uploads_removed = 0u64;
        let mut temp_removed = 0u64;

        let mut bucket_entries = match fs::read_dir(&self.buckets_dir).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("housekeeping: cannot read buckets dir: {}", e);
                return (0, 0);
            }
        };

        while let Ok(Some(bucket_entry)) = bucket_entries.next_entry().await {
            if !matches!(bucket_entry.file_type().await, Ok(ft) if ft.is_dir()) {
                continue;
            }
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
                    // Treat unreadable/missing meta as stale by directory mtime.
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

            // 2. Leftover temp files from crashed writes (bucket root level).
            temp_removed += Self::sweep_temp_files(&bucket_dir).await;
            temp_removed += Self::sweep_temp_files(&uploads_dir).await;
        }

        (uploads_removed, temp_removed)
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

    // --- Internal helpers ---
}
