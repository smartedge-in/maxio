use super::*;

impl FilesystemStorage {
    pub async fn list_buckets(&self) -> Result<Vec<BucketMeta>, StorageError> {
        let mut buckets = Vec::new();
        let mut entries = fs::read_dir(&self.buckets_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let meta_path = entry.path().join(".bucket.json");
                if let Ok(data) = fs::read_to_string(&meta_path).await {
                    if let Ok(meta) = serde_json::from_str::<BucketMeta>(&data) {
                        buckets.push(meta);
                    }
                }
            }
        }
        buckets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(buckets)
    }

    // --- Object operations ---


    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        validate_bucket_name(bucket)?;
        let bucket_dir = self.buckets_dir.join(bucket);
        let mut results = Vec::new();
        self.walk_dir(&bucket_dir, &bucket_dir, prefix, &mut results)
            .await?;
        results.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(results)
    }

    pub(super) async fn read_object_meta(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        let meta_path = self.meta_path(bucket, key);
        let data = fs::read_to_string(&meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    pub(super) fn walk_dir<'a>(
        &'a self,
        base: &'a Path,
        dir: &'a Path,
        prefix: &'a str,
        results: &'a mut Vec<ObjectMeta>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut entries = match fs::read_dir(dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.into()),
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let fname = entry.file_name().to_string_lossy().to_string();

                if fname.ends_with(".meta.json")
                    || fname == ".bucket.json"
                    || fname == ".uploads"
                    || fname == ".versions"
                    || fname == ".folder"
                {
                    continue;
                }

                // EC chunk directory: derive the object key and read its metadata
                if fname.ends_with(".ec") && entry.file_type().await?.is_dir() {
                    if let Ok(rel) = path.strip_prefix(base) {
                        let rel_str = rel.to_string_lossy();
                        // Strip the .ec suffix to get the key
                        let key = rel_str.strip_suffix(".ec").unwrap_or(&rel_str).to_string();
                        if key.starts_with(prefix) {
                            if let Ok(meta) = self
                                .read_object_meta(base.file_name().unwrap().to_str().unwrap(), &key)
                                .await
                            {
                                results.push(meta);
                            }
                        }
                    }
                    continue;
                }

                if entry.file_type().await?.is_dir() {
                    // Check for folder marker inside this directory
                    let marker = path.join(".folder.meta.json");
                    if marker.exists() {
                        if let Ok(rel) = path.strip_prefix(base) {
                            let key = format!("{}/", rel.to_string_lossy());
                            if key.starts_with(prefix) {
                                if let Ok(data) = fs::read_to_string(&marker).await {
                                    if let Ok(meta) = serde_json::from_str::<ObjectMeta>(&data) {
                                        results.push(meta);
                                    }
                                }
                            }
                        }
                    }
                    self.walk_dir(base, &path, prefix, results).await?;
                } else {
                    if let Ok(rel) = path.strip_prefix(base) {
                        let key = rel.to_string_lossy().to_string();
                        if key.starts_with(prefix) {
                            if let Ok(meta) = self
                                .read_object_meta(base.file_name().unwrap().to_str().unwrap(), &key)
                                .await
                            {
                                results.push(meta);
                            }
                        }
                    }
                }
            }
            Ok(())
        })
    }


    pub async fn list_object_versions(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<ObjectMeta>, StorageError> {
        validate_bucket_name(bucket)?;
        let bucket_dir = self.buckets_dir.join(bucket);
        let mut results = Vec::new();
        self.walk_versions(&bucket_dir, &bucket_dir, prefix, &mut results)
            .await?;
        // S3's suspended state can have a current "null" version outside
        // .versions. Include it in version listings when present.
        for obj in self.list_objects(bucket, prefix).await? {
            if obj.version_id.is_none() {
                results.push(obj);
            }
        }
        // Sort by key, then by version_id descending (newest first per key)
        results.sort_by(|a, b| {
            a.key.cmp(&b.key).then_with(|| {
                let va = a.version_id.as_deref().unwrap_or("");
                let vb = b.version_id.as_deref().unwrap_or("");
                vb.cmp(va)
            })
        });
        Ok(results)
    }

    pub(super) fn walk_versions<'a>(
        &'a self,
        base: &'a Path,
        dir: &'a Path,
        prefix: &'a str,
        results: &'a mut Vec<ObjectMeta>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut entries = match fs::read_dir(dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.into()),
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let fname = entry.file_name().to_string_lossy().to_string();

                if !entry.file_type().await?.is_dir() {
                    continue;
                }

                if fname == ".versions" {
                    // Scan all key dirs inside .versions
                    let mut key_dirs = match fs::read_dir(&path).await {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    while let Some(key_entry) = key_dirs.next_entry().await? {
                        if !key_entry.file_type().await?.is_dir() {
                            continue;
                        }
                        let key_name = key_entry.file_name().to_string_lossy().to_string();
                        // Reconstruct the object key from the directory structure
                        let parent_rel = dir.strip_prefix(base).unwrap_or(Path::new(""));
                        let key = if parent_rel.as_os_str().is_empty() {
                            key_name.clone()
                        } else {
                            format!("{}/{}", parent_rel.to_string_lossy(), key_name)
                        };
                        if !key.starts_with(prefix) {
                            continue;
                        }
                        // Read all version meta files in this key's version dir
                        let key_ver_dir = key_entry.path();
                        let mut ver_entries = match fs::read_dir(&key_ver_dir).await {
                            Ok(e) => e,
                            Err(_) => continue,
                        };
                        while let Some(ve) = ver_entries.next_entry().await? {
                            let vf = ve.file_name().to_string_lossy().to_string();
                            if vf.ends_with(".meta.json") {
                                if let Ok(data) = fs::read_to_string(ve.path()).await {
                                    if let Ok(meta) = serde_json::from_str::<ObjectMeta>(&data) {
                                        results.push(meta);
                                    }
                                }
                            }
                        }
                    }
                } else if fname != ".uploads" && fname != ".bucket.json" {
                    self.walk_versions(base, &path, prefix, results).await?;
                }
            }
            Ok(())
        })
    }
}
