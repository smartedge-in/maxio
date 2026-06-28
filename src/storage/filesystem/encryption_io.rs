use super::*;

impl FilesystemStorage {
    pub(super) fn prepare_encryption(&self, req: &EncryptionRequest) -> Result<EncryptionMeta, StorageError> {
        let b64 = base64::engine::general_purpose::STANDARD;
        match req.mode {
            EncryptionMode::SseS3 => {
                let dek = Keyring::generate_dek();
                let nonce_prefix = Keyring::generate_nonce_prefix8();
                let key_id = self.keyring.active_id().to_string();
                let (wrapped_dek, wrap_nonce) = self
                    .keyring
                    .wrap_dek(&key_id, &dek)
                    .map_err(|e| StorageError::EncryptionError(e.to_string()))?;
                Ok(EncryptionMeta {
                    algorithm: "AES256".to_string(),
                    mode: EncryptionMode::SseS3,
                    key_id: Some(key_id),
                    wrapped_dek: Some(b64.encode(&wrapped_dek)),
                    wrap_nonce: Some(b64.encode(wrap_nonce)),
                    customer_key_md5: None,
                    nonce_prefix: b64.encode(nonce_prefix),
                    chunk_size: FRAME_CHUNK_SIZE as u32,
                    sidecar_mac: String::new(),
                })
            }
            EncryptionMode::SseC => {
                let customer_key = req.customer_key.as_ref().ok_or_else(|| {
                    StorageError::EncryptionError("SSE-C requires customer key".into())
                })?;
                let dek = Keyring::generate_dek();
                let nonce_prefix = Keyring::generate_nonce_prefix8();
                let md5 = Md5::digest(&**customer_key);
                let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&**customer_key));
                let mut wrap_nonce = [0u8; 12];
                rand::rng().fill(&mut wrap_nonce[..]);
                let wrapped_dek = cipher
                    .encrypt(Nonce::from_slice(&wrap_nonce), dek.as_slice())
                    .map_err(|_| {
                        StorageError::EncryptionError("SSE-C DEK wrapping failed".into())
                    })?;
                Ok(EncryptionMeta {
                    algorithm: "AES256".to_string(),
                    mode: EncryptionMode::SseC,
                    key_id: None,
                    wrapped_dek: Some(b64.encode(&wrapped_dek)),
                    wrap_nonce: Some(b64.encode(wrap_nonce)),
                    customer_key_md5: Some(b64.encode(md5)),
                    nonce_prefix: b64.encode(nonce_prefix),
                    chunk_size: FRAME_CHUNK_SIZE as u32,
                    sidecar_mac: String::new(),
                })
            }
        }
    }

    /// Resolve the upload-scoped DEK for a multipart upload. SSE-S3 unwraps the
    /// stored wrapped DEK via the active keyring. SSE-C derives the DEK from the
    /// customer key supplied on each `UploadPart` / `Complete` call, and rejects
    /// mismatched keys (MD5 compared against the value pinned at
    /// `CreateMultipartUpload`).
    pub(super) fn resolve_upload_dek(
        &self,
        spec: &UploadEncryptionSpec,
        customer_key: Option<[u8; 32]>,
    ) -> Result<[u8; 32], StorageError> {
        let b64 = base64::engine::general_purpose::STANDARD;
        match spec.mode {
            EncryptionMode::SseC => {
                let ck = customer_key.ok_or_else(|| {
                    StorageError::EncryptionError("SSE-C multipart: customer key required".into())
                })?;
                if let Some(ref stored) = spec.customer_key_md5 {
                    let provided_md5 = Md5::digest(ck);
                    if b64.encode(provided_md5) != *stored {
                        return Err(StorageError::EncryptionError(
                            "SSE-C key MD5 mismatch".into(),
                        ));
                    }
                }
                Ok(ck)
            }
            EncryptionMode::SseS3 => {
                let wrapped_b64 = spec.upload_dek_wrapped.as_ref().ok_or_else(|| {
                    StorageError::EncryptionError("missing upload_dek_wrapped".into())
                })?;
                let nonce_b64 = spec.upload_dek_wrap_nonce.as_ref().ok_or_else(|| {
                    StorageError::EncryptionError("missing upload_dek_wrap_nonce".into())
                })?;
                let kid = spec.upload_dek_key_id.as_ref().ok_or_else(|| {
                    StorageError::EncryptionError("missing upload_dek_key_id".into())
                })?;
                let wrapped = b64.decode(wrapped_b64).map_err(|_| {
                    StorageError::EncryptionError("bad upload_dek_wrapped base64".into())
                })?;
                let nonce_bytes = b64.decode(nonce_b64).map_err(|_| {
                    StorageError::EncryptionError("bad upload_dek_wrap_nonce base64".into())
                })?;
                if nonce_bytes.len() != 12 {
                    return Err(StorageError::EncryptionError(
                        "upload_dek_wrap_nonce must be 12 bytes".into(),
                    ));
                }
                let mut nonce_arr = [0u8; 12];
                nonce_arr.copy_from_slice(&nonce_bytes);
                self.keyring
                    .unwrap_dek(kid, &wrapped, &nonce_arr)
                    .map_err(|e| StorageError::EncryptionError(e.to_string()))
            }
        }
    }

    pub(super) fn resolve_dek(
        &self,
        enc_meta: &EncryptionMeta,
        customer_key: Option<[u8; 32]>,
    ) -> Result<[u8; 32], StorageError> {
        let b64 = base64::engine::general_purpose::STANDARD;
        match enc_meta.mode {
            EncryptionMode::SseC => {
                let ck = customer_key.ok_or_else(|| {
                    StorageError::DecryptionError("SSE-C: customer key required".into())
                })?;
                // Validate MD5 if recorded.
                if let Some(ref stored_md5_b64) = enc_meta.customer_key_md5 {
                    let provided_md5 = Md5::digest(&ck);
                    let provided_b64 = b64.encode(provided_md5);
                    if &provided_b64 != stored_md5_b64 {
                        return Err(StorageError::DecryptionError(
                            "SSE-C: customer key MD5 mismatch".into(),
                        ));
                    }
                }
                let (Some(wrapped), Some(wrap_nonce)) =
                    (enc_meta.wrapped_dek.as_ref(), enc_meta.wrap_nonce.as_ref())
                else {
                    // Legacy MaxIO SSE-C objects used the customer key directly.
                    return Ok(ck);
                };
                let wrapped_bytes = b64
                    .decode(wrapped)
                    .map_err(|_| StorageError::DecryptionError("bad wrapped_dek base64".into()))?;
                let nonce_bytes = b64
                    .decode(wrap_nonce)
                    .map_err(|_| StorageError::DecryptionError("bad wrap_nonce base64".into()))?;
                if nonce_bytes.len() != 12 {
                    return Err(StorageError::DecryptionError(
                        "wrap_nonce must be 12 bytes".into(),
                    ));
                }
                let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&ck));
                let plaintext = cipher
                    .decrypt(Nonce::from_slice(&nonce_bytes), wrapped_bytes.as_slice())
                    .map_err(|_| StorageError::DecryptionError("SSE-C DEK unwrap failed".into()))?;
                if plaintext.len() != 32 {
                    return Err(StorageError::DecryptionError(
                        "SSE-C DEK length invalid".into(),
                    ));
                }
                let mut dek = [0u8; 32];
                dek.copy_from_slice(&plaintext);
                Ok(dek)
            }
            EncryptionMode::SseS3 => {
                let key_id = enc_meta
                    .key_id
                    .as_ref()
                    .ok_or_else(|| StorageError::DecryptionError("missing key_id".into()))?;
                let wrapped = enc_meta
                    .wrapped_dek
                    .as_ref()
                    .ok_or_else(|| StorageError::DecryptionError("missing wrapped_dek".into()))?;
                let wrap_nonce = enc_meta
                    .wrap_nonce
                    .as_ref()
                    .ok_or_else(|| StorageError::DecryptionError("missing wrap_nonce".into()))?;
                let wrapped_bytes = b64
                    .decode(wrapped)
                    .map_err(|_| StorageError::DecryptionError("bad wrapped_dek base64".into()))?;
                let nonce_bytes = b64
                    .decode(wrap_nonce)
                    .map_err(|_| StorageError::DecryptionError("bad wrap_nonce base64".into()))?;
                if nonce_bytes.len() != 12 {
                    return Err(StorageError::DecryptionError(
                        "wrap_nonce must be 12 bytes".into(),
                    ));
                }
                let mut nonce_arr = [0u8; 12];
                nonce_arr.copy_from_slice(&nonce_bytes);
                self.keyring
                    .unwrap_dek(key_id, &wrapped_bytes, &nonce_arr)
                    .map_err(|e| StorageError::DecryptionError(e.to_string()))
            }
        }
    }

    /// Write a new version to the `.versions/` directory and update the current (top-level) files.
    pub(super) async fn write_version(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
        data_path: &Path,
    ) -> Result<(), StorageError> {
        let version_id = meta.version_id.as_ref().unwrap();
        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;

        // Copy data to version store
        let ver_data = ver_dir.join(format!("{}.data", version_id));
        fs::copy(data_path, &ver_data).await?;

        // Write version metadata
        let ver_meta = ver_dir.join(format!("{}.meta.json", version_id));
        fs::write(&ver_meta, serde_json::to_string_pretty(meta)?).await?;

        Ok(())
    }

    /// Write a new chunked version: copy .ec/ dir to .versions/{key}/{version_id}.ec/
    pub(super) async fn write_version_chunked(
        &self,
        bucket: &str,
        key: &str,
        meta: &ObjectMeta,
    ) -> Result<(), StorageError> {
        let version_id = meta.version_id.as_ref().unwrap();
        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;

        // Copy the entire .ec/ directory
        let src_ec = self.ec_dir(bucket, key);
        let dst_ec = ver_dir.join(format!("{}.ec", version_id));
        fs::create_dir_all(&dst_ec).await?;
        let mut entries = fs::read_dir(&src_ec).await?;
        while let Some(entry) = entries.next_entry().await? {
            let dest = dst_ec.join(entry.file_name());
            fs::copy(entry.path(), &dest).await?;
        }

        // Write version metadata
        let ver_meta = ver_dir.join(format!("{}.meta.json", version_id));
        fs::write(&ver_meta, serde_json::to_string_pretty(meta)?).await?;

        Ok(())
    }

    /// Write a delete marker version and remove the top-level files.
    pub(super) async fn write_delete_marker(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<DeleteResult, StorageError> {
        let version_id = Self::generate_version_id();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let marker_meta = ObjectMeta {
            key: key.to_string(),
            size: 0,
            etag: String::new(),
            content_type: String::new(),
            last_modified: now,
            version_id: Some(version_id.clone()),
            is_delete_marker: true,
            storage_format: None,
            checksum_algorithm: None,
            checksum_value: None,
            tags: None,
            part_sizes: None,
            encryption: None,
        };

        let ver_dir = self.versions_dir(bucket, key);
        fs::create_dir_all(&ver_dir).await?;
        let ver_meta_path = ver_dir.join(format!("{}.meta.json", version_id));
        fs::write(&ver_meta_path, serde_json::to_string_pretty(&marker_meta)?).await?;

        // Remove top-level current files
        let _ = fs::remove_file(self.object_path(bucket, key)).await;
        let _ = fs::remove_file(self.meta_path(bucket, key)).await;
        let _ = fs::remove_dir_all(self.ec_dir(bucket, key)).await;

        Ok(DeleteResult {
            version_id: Some(version_id),
            is_delete_marker: true,
        })
    }

    /// Scan versions for a key and update the top-level files to reflect the latest non-delete-marker.
    pub(super) async fn update_current_version(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let ver_dir = self.versions_dir(bucket, key);
        if !fs::try_exists(&ver_dir).await.unwrap_or(false) {
            return Ok(());
        }

        // Find the latest non-delete-marker version (lexicographic sort = chronological)
        let mut versions = Vec::new();
        let mut entries = fs::read_dir(&ver_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".meta.json") {
                versions.push(fname);
            }
        }
        versions.sort();
        versions.reverse(); // newest first

        for meta_fname in &versions {
            let meta_path = ver_dir.join(meta_fname);
            let data = fs::read_to_string(&meta_path).await?;
            let meta: ObjectMeta = serde_json::from_str(&data)?;
            if !meta.is_delete_marker {
                // Restore this version as current
                let vid = meta.version_id.as_ref().unwrap();
                let obj_meta_path = self.meta_path(bucket, key);

                let ver_ec = ver_dir.join(format!("{}.ec", vid));
                if ver_ec.is_dir() {
                    // Restore chunked version
                    let dst_ec = self.ec_dir(bucket, key);
                    if let Some(parent) = dst_ec.parent() {
                        fs::create_dir_all(parent).await?;
                    }
                    let _ = fs::remove_dir_all(&dst_ec).await;
                    fs::create_dir_all(&dst_ec).await?;
                    let mut entries = fs::read_dir(&ver_ec).await?;
                    while let Some(entry) = entries.next_entry().await? {
                        fs::copy(entry.path(), dst_ec.join(entry.file_name())).await?;
                    }
                } else {
                    // Restore flat version
                    let ver_data = ver_dir.join(format!("{}.data", vid));
                    let obj_path = self.object_path(bucket, key);
                    if let Some(parent) = obj_path.parent() {
                        fs::create_dir_all(parent).await?;
                    }
                    fs::copy(&ver_data, &obj_path).await?;
                }

                if let Some(parent) = obj_meta_path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                fs::write(&obj_meta_path, serde_json::to_string_pretty(&meta)?).await?;
                return Ok(());
            }
        }

        // All versions are delete markers — remove top-level files
        let _ = fs::remove_file(self.object_path(bucket, key)).await;
        let _ = fs::remove_file(self.meta_path(bucket, key)).await;
        let _ = fs::remove_dir_all(self.ec_dir(bucket, key)).await;
        Ok(())
    }

    pub async fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        if version_id == "null" {
            return self.get_object(bucket, key, customer_key).await;
        }
        let ver_meta_path = self.version_meta_path(bucket, key, version_id);
        let data = fs::read_to_string(&ver_meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: ObjectMeta = serde_json::from_str(&data)?;

        if meta.is_delete_marker {
            return Err(StorageError::NotFound(key.to_string()));
        }
        reject_sse_c_on_plaintext(&meta, customer_key.is_some())?;

        // Check for chunked version
        let ver_ec_dir = self
            .versions_dir(bucket, key)
            .join(format!("{}.ec", version_id));
        if ver_ec_dir.is_dir() {
            let manifest_path = ver_ec_dir.join("manifest.json");
            let manifest_data = fs::read_to_string(&manifest_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::VersionNotFound(version_id.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            let manifest: ChunkManifest = serde_json::from_str(&manifest_data)?;
            if let Some(ref enc_meta) = meta.encryption {
                let dek = self.resolve_dek(enc_meta, customer_key)?;
                verify_sidecar_mac(&meta, &dek)?;
                let frame_size = enc_meta.chunk_size as usize;
                let plaintext_size = meta.size;
                let aad_builder = object_aad_builder(bucket, key, meta.version_id.as_deref());
                let ct_reader = VerifiedChunkReader::new(ver_ec_dir, manifest);
                let decryptor = FrameDecryptor::new(
                    Box::pin(ct_reader),
                    &dek,
                    plaintext_size,
                    frame_size,
                    aad_builder,
                );
                return Ok((Box::pin(decryptor), meta));
            }
            let reader = VerifiedChunkReader::new(ver_ec_dir, manifest);
            return Ok((Box::pin(reader), meta));
        }

        let ver_data_path = self.version_data_path(bucket, key, version_id);

        // Encrypted version — resolve DEK, verify sidecar MAC, wrap in
        // FrameDecryptor with the same AAD scheme used for live GET so a
        // version file cannot be silently swapped across objects.
        if let Some(ref enc_meta) = meta.encryption {
            let dek = self.resolve_dek(enc_meta, customer_key)?;
            verify_sidecar_mac(&meta, &dek)?;
            let chunk_size = enc_meta.chunk_size as usize;
            let plaintext_size = meta.size;
            let file = fs::File::open(&ver_data_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::VersionNotFound(version_id.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            let aad_builder = object_aad_builder(bucket, key, meta.version_id.as_deref());
            let decryptor = FrameDecryptor::new(
                Box::pin(file),
                &dek,
                plaintext_size,
                chunk_size,
                aad_builder,
            );
            return Ok((Box::pin(decryptor), meta));
        }

        let file = fs::File::open(&ver_data_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok((
            Box::pin(BufReader::with_capacity(IO_BUFFER_SIZE, file)),
            meta,
        ))
    }

    pub async fn head_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        if version_id == "null" {
            return self.head_object(bucket, key).await;
        }
        let ver_meta_path = self.version_meta_path(bucket, key, version_id);
        let data = fs::read_to_string(&ver_meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: ObjectMeta = serde_json::from_str(&data)?;
        if meta.is_delete_marker {
            return Err(StorageError::NotFound(key.to_string()));
        }
        Ok(meta)
    }

    pub async fn delete_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        if version_id == "null" {
            let meta = self.read_object_meta(bucket, key).await?;
            remove_file_if_exists(&self.object_path(bucket, key)).await?;
            remove_file_if_exists(&self.meta_path(bucket, key)).await?;
            remove_dir_all_if_exists(&self.ec_dir(bucket, key)).await?;
            self.update_current_version(bucket, key).await?;
            return Ok(meta);
        }
        let ver_meta_path = self.version_meta_path(bucket, key, version_id);
        let data = fs::read_to_string(&ver_meta_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::VersionNotFound(version_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let meta: ObjectMeta = serde_json::from_str(&data)?;

        // Remove version files
        let _ = fs::remove_file(&ver_meta_path).await;
        let ver_data_path = self.version_data_path(bucket, key, version_id);
        let _ = fs::remove_file(&ver_data_path).await;
        let ver_ec_dir = self
            .versions_dir(bucket, key)
            .join(format!("{}.ec", version_id));
        let _ = fs::remove_dir_all(&ver_ec_dir).await;

        // Clean up empty versions dir
        let ver_dir = self.versions_dir(bucket, key);
        let _ = fs::remove_dir(&ver_dir).await; // only succeeds if empty

        // Update current version (in case we deleted the latest or a delete marker)
        self.update_current_version(bucket, key).await?;

        Ok(meta)
    }

}
