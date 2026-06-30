use super::*;

trait ChunkInfoExt {
    fn into_parity(self) -> ChunkInfo;
}

impl ChunkInfoExt for ChunkInfo {
    fn into_parity(mut self) -> ChunkInfo {
        self.kind = ChunkKind::Parity;
        self
    }
}

impl FilesystemStorage {
    #[allow(clippy::too_many_arguments)] // S3 PutObject surface: bucket, key, body, checksum, encryption, size
    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
        encryption: Option<EncryptionRequest>,
        declared_size: Option<u64>,
    ) -> Result<PutResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        self.ensure_can_overwrite(bucket, key).await?;
        self.check_upload_start(declared_size)?;
        let mut body = self.wrap_upload_reader(body);

        // Folder marker: zero-byte object with key ending in /
        if key.ends_with('/') {
            return self.put_folder_marker(bucket, key).await;
        }

        if self.effective_erasure_coding(bucket).await? {
            if let Some(req) = encryption {
                return self
                    .put_object_chunked_encrypted(bucket, key, content_type, body, checksum, req)
                    .await;
            }
            return self
                .put_object_chunked(
                    bucket,
                    key,
                    content_type,
                    body,
                    checksum.as_ref().map(|(a, _)| *a),
                )
                .await;
        }

        // Determine version_id up front so it can be folded into the AAD.
        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        // Prepare encryption metadata and cipher
        let mut enc_meta_opt: Option<EncryptionMeta> = match encryption {
            Some(ref req) => Some(
                self.prepare_encryption(req)
                    .map_err(|e| StorageError::EncryptionError(e.to_string()))?,
            ),
            None => None,
        };
        let (cipher_opt, nonce_prefix, dek_opt) = if let Some(ref em) = enc_meta_opt {
            let dek = self
                .resolve_dek(
                    em,
                    encryption
                        .as_ref()
                        .and_then(|r| r.customer_key.as_ref().map(|k| **k)),
                )
                .map_err(|e| StorageError::EncryptionError(e.to_string()))?;
            let b64 = base64::engine::general_purpose::STANDARD;
            let prefix_bytes = b64
                .decode(&em.nonce_prefix)
                .map_err(|_| StorageError::EncryptionError("invalid nonce_prefix".into()))?;
            let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
            (Some(cipher), prefix_bytes, Some(dek))
        } else {
            (None, Vec::new(), None)
        };

        let obj_path = self.object_path(bucket, key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let tmp_obj_path = temp_sibling_path(&obj_path);
        let mut tmp_obj_guard = TempPathGuard::file(tmp_obj_path.clone());
        let file = fs::File::create(&tmp_obj_path).await?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, file);
        let mut hasher = Md5::new();
        let mut checksum_hasher = checksum
            .as_ref()
            .map(|(algo, _)| ChecksumHasher::new(*algo));
        let mut size: u64 = 0;
        let mut buf = vec![0u8; IO_BUFFER_SIZE];
        let mut frame_buf: Vec<u8> = Vec::with_capacity(FRAME_CHUNK_SIZE);
        let mut chunk_index: u64 = 0;

        loop {
            let n = body.read(&mut buf).await.map_err(map_read_quota_error)?;
            if n == 0 {
                // flush remaining partial frame
                if let Some(ref cipher) = cipher_opt
                    && !frame_buf.is_empty()
                {
                    let aad = build_frame_aad(bucket, key, version_id.as_deref(), chunk_index);
                    write_encrypted_frame(
                        &mut writer,
                        cipher,
                        &nonce_prefix,
                        chunk_index,
                        &frame_buf,
                        &aad,
                    )
                    .await?;
                }
                break;
            }
            hasher.update(&buf[..n]);
            if let Some(ref mut ch) = checksum_hasher {
                ch.update(&buf[..n]);
            }
            size += n as u64;
            if let Some(ref cipher) = cipher_opt {
                frame_buf.extend_from_slice(&buf[..n]);
                while frame_buf.len() >= FRAME_CHUNK_SIZE {
                    let frame_data: Vec<u8> = frame_buf.drain(..FRAME_CHUNK_SIZE).collect();
                    let aad = build_frame_aad(bucket, key, version_id.as_deref(), chunk_index);
                    write_encrypted_frame(
                        &mut writer,
                        cipher,
                        &nonce_prefix,
                        chunk_index,
                        &frame_data,
                        &aad,
                    )
                    .await?;
                    chunk_index += 1;
                }
            } else {
                writer.write_all(&buf[..n]).await?;
            }
        }
        writer.flush().await?;

        let etag = hex::encode(hasher.finalize());
        let etag_quoted = format!("\"{}\"", etag);

        // Validate and compute checksum
        let (checksum_algorithm, checksum_value) = if let Some((algo, expected)) = checksum {
            let Some(hasher) = checksum_hasher else {
                return Err(StorageError::IntegrityError(
                    "checksum validation enabled but hasher missing".into(),
                ));
            };
            let computed = hasher.finalize_base64();
            if let Some(expected_val) = expected
                && computed != expected_val
            {
                let _ = fs::remove_file(&tmp_obj_path).await;
                return Err(StorageError::ChecksumMismatch(format!(
                    "expected {}, got {}",
                    expected_val, computed
                )));
            }
            (Some(algo), Some(computed))
        } else {
            (None, None)
        };

        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        // Fold the sidecar MAC into the encryption metadata now that every
        // immutable field (size/etag/version_id/etc.) is final.
        let default_lock = self.resolve_default_object_lock(bucket).await?;
        let mut meta = ObjectMeta {
            key: key.to_string(),
            size,
            etag: etag_quoted.clone(),
            content_type: content_type.to_string(),
            last_modified: now,
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: None,
            encryption: enc_meta_opt.take(),
            object_lock_mode: None,
            retain_until_date: None,
            legal_hold_status: None,
        };
        Self::apply_object_lock(&mut meta, default_lock);
        if meta.encryption.is_some() {
            if let Some(em) = meta.encryption.as_mut() {
                em.sidecar_mac.clear();
            }
            if let Some(dek) = dek_opt.as_ref() {
                let mac = compute_sidecar_mac(dek, &meta)?;
                if let Some(em) = meta.encryption.as_mut() {
                    em.sidecar_mac = mac;
                }
            }
        }

        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(&meta)?;
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, json).await?;
        publish_temp_payload_and_meta(&tmp_obj_path, &obj_path, false, &tmp_meta_path, &meta_path)
            .await?;
        tmp_obj_guard.disarm();
        tmp_meta_guard.disarm();

        if versioned {
            self.write_version(bucket, key, &meta, &obj_path).await?;
        }

        self.index_upsert(bucket, &meta);
        Ok(PutResult {
            size,
            etag: etag_quoted,
            version_id,
            checksum_algorithm,
            checksum_value,
        })
    }

    pub(super) async fn put_object_chunked(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        mut body: ByteStream,
        checksum_algo: Option<ChecksumAlgorithm>,
    ) -> Result<PutResult, StorageError> {
        validate_bucket_name(bucket)?;
        let ec_dir = self.ec_dir(bucket, key);
        let tmp_ec_dir = temp_sibling_path(&ec_dir);
        let mut tmp_ec_guard = TempPathGuard::dir(tmp_ec_dir.clone());
        if let Some(parent) = ec_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::create_dir_all(&tmp_ec_dir).await?;

        let mut md5_hasher = Md5::new();
        let mut checksum_hasher = checksum_algo.map(ChecksumHasher::new);
        let mut total_size: u64 = 0;
        let mut chunks: Vec<ChunkInfo> = Vec::new();
        let mut chunk_index: u32 = 0;

        let mut read_buf = vec![0u8; IO_BUFFER_SIZE];
        let mut chunk_buf = Vec::with_capacity(self.chunk_size as usize);

        loop {
            let n = body
                .read(&mut read_buf)
                .await
                .map_err(map_read_quota_error)?;
            if n == 0 {
                // Flush remaining chunk_buf
                if !chunk_buf.is_empty() {
                    let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_buf).await?;
                    chunks.push(ci);
                }
                break;
            }

            md5_hasher.update(&read_buf[..n]);
            if let Some(ref mut ch) = checksum_hasher {
                ch.update(&read_buf[..n]);
            }
            total_size += n as u64;
            chunk_buf.extend_from_slice(&read_buf[..n]);

            while chunk_buf.len() >= self.chunk_size as usize {
                let chunk_data: Vec<u8> = chunk_buf.drain(..self.chunk_size as usize).collect();
                let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_data).await?;
                chunks.push(ci);
                chunk_index += 1;
            }
        }

        // Handle empty object (zero chunks)
        if chunks.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, 0, &[]).await?;
            chunks.push(ci);
        }

        let data_chunk_count = chunks.len() as u32;

        // Compute and write parity shards if configured (skip for empty objects)
        let has_parity = self.parity_shards > 0 && total_size > 0;
        if has_parity {
            let parity_infos = self
                .compute_and_write_parity_in_dir(&tmp_ec_dir, &chunks)
                .await?;
            chunks.extend(parity_infos);
        }

        let manifest = ChunkManifest {
            version: if has_parity { 2 } else { 1 },
            total_size,
            chunk_size: self.chunk_size,
            chunk_count: data_chunk_count,
            chunks,
            parity_shards: if has_parity {
                Some(self.parity_shards)
            } else {
                None
            },
            shard_size: if has_parity {
                Some(self.chunk_size)
            } else {
                None
            },
            plaintext_size: None,
        };
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        fs::write(tmp_ec_dir.join("manifest.json"), manifest_json).await?;

        let etag = hex::encode(md5_hasher.finalize());
        let etag_quoted = format!("\"{}\"", etag);
        let checksum_value = checksum_hasher.map(|h| h.finalize_base64());

        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        let storage_format = if has_parity {
            "chunked-v2"
        } else {
            "chunked-v1"
        };
        let meta = ObjectMeta {
            key: key.to_string(),
            size: total_size,
            etag: etag_quoted.clone(),
            content_type: content_type.to_string(),
            last_modified: now,
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: Some(storage_format.to_string()),
            checksum_algorithm: checksum_algo,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: None,
            encryption: None,
            object_lock_mode: None,
            retain_until_date: None,
            legal_hold_status: None,
        };

        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, serde_json::to_string_pretty(&meta)?).await?;
        publish_temp_payload_and_meta(&tmp_ec_dir, &ec_dir, true, &tmp_meta_path, &meta_path)
            .await?;
        tmp_ec_guard.disarm();
        tmp_meta_guard.disarm();

        if versioned {
            self.write_version_chunked(bucket, key, &meta).await?;
        }

        self.index_upsert(bucket, &meta);
        Ok(PutResult {
            size: total_size,
            etag: etag_quoted,
            version_id,
            checksum_algorithm: checksum_algo,
            checksum_value,
        })
    }

    /// Encrypt-then-EC write path. Frames plaintext through AES-256-GCM (reusing
    /// the same 64 KiB frame format as non-EC SSE), then chunks the ciphertext
    /// stream into `self.chunk_size`-sized EC chunks. Frame boundaries are not
    /// aligned with chunk boundaries — RS reconstructs chunk bytes byte-exact,
    /// so frames re-emerge intact on read.
    pub(super) async fn put_object_chunked_encrypted(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        mut body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
        encryption: EncryptionRequest,
    ) -> Result<PutResult, StorageError> {
        validate_bucket_name(bucket)?;
        let ec_dir = self.ec_dir(bucket, key);
        let tmp_ec_dir = temp_sibling_path(&ec_dir);
        let mut tmp_ec_guard = TempPathGuard::dir(tmp_ec_dir.clone());
        if let Some(parent) = ec_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::create_dir_all(&tmp_ec_dir).await?;

        // Version-id upfront: AAD binds to it, so we need it before the first
        // frame is encrypted.
        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        let enc_meta = self
            .prepare_encryption(&encryption)
            .map_err(|e| StorageError::EncryptionError(e.to_string()))?;
        let dek = self
            .resolve_dek(&enc_meta, encryption.customer_key.as_ref().map(|k| **k))
            .map_err(|e| StorageError::EncryptionError(e.to_string()))?;
        let b64 = base64::engine::general_purpose::STANDARD;
        let prefix_bytes = b64
            .decode(&enc_meta.nonce_prefix)
            .map_err(|_| StorageError::EncryptionError("invalid nonce_prefix".into()))?;
        let nonce_prefix = prefix_bytes;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));

        let checksum_algo = checksum.as_ref().map(|(a, _)| *a);
        let expected_checksum = checksum.as_ref().and_then(|(_, v)| v.clone());
        let mut md5_hasher = Md5::new();
        let mut checksum_hasher = checksum_algo.map(ChecksumHasher::new);
        let mut plaintext_size: u64 = 0;
        let mut ct_size: u64 = 0;
        let mut chunks: Vec<ChunkInfo> = Vec::new();
        let mut chunk_index: u32 = 0;
        let mut frame_index: u64 = 0;
        let mut read_buf = vec![0u8; IO_BUFFER_SIZE];
        let mut frame_buf: Vec<u8> = Vec::with_capacity(FRAME_CHUNK_SIZE);
        let mut chunk_buf: Vec<u8> = Vec::with_capacity(self.chunk_size as usize);

        loop {
            let n = body
                .read(&mut read_buf)
                .await
                .map_err(map_read_quota_error)?;
            if n == 0 {
                // Flush trailing partial frame.
                if !frame_buf.is_empty() {
                    let aad = build_frame_aad(bucket, key, version_id.as_deref(), frame_index);
                    let ct = encrypt_frame_to_vec(
                        &cipher,
                        &nonce_prefix,
                        frame_index,
                        &frame_buf,
                        &aad,
                    )?;
                    chunk_buf.extend_from_slice(&ct);
                    frame_buf.clear();
                }
                // Flush full chunks then any remainder.
                while chunk_buf.len() >= self.chunk_size as usize {
                    let chunk_data: Vec<u8> = chunk_buf.drain(..self.chunk_size as usize).collect();
                    ct_size += chunk_data.len() as u64;
                    let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_data).await?;
                    chunks.push(ci);
                    chunk_index += 1;
                }
                if !chunk_buf.is_empty() {
                    ct_size += chunk_buf.len() as u64;
                    let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_buf).await?;
                    chunks.push(ci);
                    chunk_buf.clear();
                }
                break;
            }
            md5_hasher.update(&read_buf[..n]);
            if let Some(ref mut ch) = checksum_hasher {
                ch.update(&read_buf[..n]);
            }
            plaintext_size += n as u64;
            frame_buf.extend_from_slice(&read_buf[..n]);
            while frame_buf.len() >= FRAME_CHUNK_SIZE {
                let frame_data: Vec<u8> = frame_buf.drain(..FRAME_CHUNK_SIZE).collect();
                let aad = build_frame_aad(bucket, key, version_id.as_deref(), frame_index);
                let ct =
                    encrypt_frame_to_vec(&cipher, &nonce_prefix, frame_index, &frame_data, &aad)?;
                chunk_buf.extend_from_slice(&ct);
                frame_index += 1;
                while chunk_buf.len() >= self.chunk_size as usize {
                    let chunk_data: Vec<u8> = chunk_buf.drain(..self.chunk_size as usize).collect();
                    ct_size += chunk_data.len() as u64;
                    let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_data).await?;
                    chunks.push(ci);
                    chunk_index += 1;
                }
            }
        }

        // Preserve the existing EC invariant: at least one chunk on disk so the
        // manifest/chunk-reader path is consistent even for empty objects.
        if chunks.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, 0, &[]).await?;
            chunks.push(ci);
        }

        let data_chunk_count = chunks.len() as u32;

        let has_parity = self.parity_shards > 0 && ct_size > 0;
        if has_parity {
            let parity_infos = self
                .compute_and_write_parity_in_dir(&tmp_ec_dir, &chunks)
                .await?;
            chunks.extend(parity_infos);
        }

        let manifest = ChunkManifest {
            version: if has_parity { 2 } else { 1 },
            total_size: ct_size,
            chunk_size: self.chunk_size,
            chunk_count: data_chunk_count,
            chunks,
            parity_shards: if has_parity {
                Some(self.parity_shards)
            } else {
                None
            },
            shard_size: if has_parity {
                Some(self.chunk_size)
            } else {
                None
            },
            plaintext_size: Some(plaintext_size),
        };
        fs::write(
            tmp_ec_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )
        .await?;

        let etag = hex::encode(md5_hasher.finalize());
        let etag_quoted = format!("\"{}\"", etag);

        let (ck_algo, ck_val) = if let Some(algo) = checksum_algo {
            let Some(hasher) = checksum_hasher else {
                return Err(StorageError::IntegrityError(
                    "checksum validation enabled but hasher missing".into(),
                ));
            };
            let computed = hasher.finalize_base64();
            if let Some(expected) = expected_checksum
                && computed != expected
            {
                let _ = fs::remove_dir_all(&tmp_ec_dir).await;
                return Err(StorageError::ChecksumMismatch(format!(
                    "expected {}, got {}",
                    expected, computed
                )));
            }
            (Some(algo), Some(computed))
        } else {
            (None, None)
        };

        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let storage_format = if has_parity {
            "chunked-v2"
        } else {
            "chunked-v1"
        };

        let mut meta = ObjectMeta {
            key: key.to_string(),
            size: plaintext_size,
            etag: etag_quoted.clone(),
            content_type: content_type.to_string(),
            last_modified: now,
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: Some(storage_format.to_string()),
            checksum_algorithm: ck_algo,
            checksum_value: ck_val.clone(),
            tags: None,
            part_sizes: None,
            encryption: Some(enc_meta),
            object_lock_mode: None,
            retain_until_date: None,
            legal_hold_status: None,
        };
        if let Some(em) = meta.encryption.as_mut() {
            em.sidecar_mac.clear();
        }
        let mac = compute_sidecar_mac(&dek, &meta)?;
        if let Some(em) = meta.encryption.as_mut() {
            em.sidecar_mac = mac;
        }

        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, serde_json::to_string_pretty(&meta)?).await?;
        publish_temp_payload_and_meta(&tmp_ec_dir, &ec_dir, true, &tmp_meta_path, &meta_path)
            .await?;
        tmp_ec_guard.disarm();
        tmp_meta_guard.disarm();

        if versioned {
            self.write_version_chunked(bucket, key, &meta).await?;
        }

        self.index_upsert(bucket, &meta);
        Ok(PutResult {
            size: plaintext_size,
            etag: etag_quoted,
            version_id,
            checksum_algorithm: ck_algo,
            checksum_value: ck_val,
        })
    }

    pub(super) async fn compute_and_write_parity_in_dir(
        &self,
        dir: &Path,
        data_chunks: &[ChunkInfo],
    ) -> Result<Vec<ChunkInfo>, StorageError> {
        self.compute_and_write_parity_from(dir, data_chunks).await
    }

    pub(super) async fn compute_and_write_parity_from(
        &self,
        dir: &Path,
        data_chunks: &[ChunkInfo],
    ) -> Result<Vec<ChunkInfo>, StorageError> {
        use reed_solomon_erasure::galois_8::ReedSolomon;

        let k = data_chunks.len();
        let m = self.parity_shards as usize;

        if k + m > 255 {
            return Err(StorageError::InvalidKey(format!(
                "too many shards: {} data + {} parity = {} > 255 (GF(2^8) limit). Increase --chunk-size",
                k,
                m,
                k + m
            )));
        }

        let shard_size = self.chunk_size as usize;
        let mut all_shards: Vec<Vec<u8>> = Vec::with_capacity(k + m);
        for ci in data_chunks {
            let path = dir.join(format!("{:06}", ci.index));
            let mut data = std::fs::read(&path).map_err(StorageError::Io)?;
            data.resize(shard_size, 0u8);
            all_shards.push(data);
        }
        for _ in 0..m {
            all_shards.push(vec![0u8; shard_size]);
        }
        let rs = ReedSolomon::new(k, m)
            .map_err(|e| StorageError::InvalidKey(format!("Reed-Solomon init error: {e}")))?;
        rs.encode(&mut all_shards)
            .map_err(|e| StorageError::InvalidKey(format!("Reed-Solomon encode error: {e}")))?;

        let mut parity_infos = Vec::with_capacity(m);
        for i in 0..m {
            let parity_index = k as u32 + i as u32;
            let shard = &all_shards[k + i];
            let path = dir.join(format!("{:06}", parity_index));
            parity_infos.push(
                write_chunk_file(&path, parity_index, shard)
                    .await?
                    .into_parity(),
            );
        }
        Ok(parity_infos)
    }

    pub async fn get_object(
        &self,
        bucket: &str,
        key: &str,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let meta = self.read_object_meta(bucket, key).await?;
        reject_sse_c_on_plaintext(&meta, customer_key.is_some())?;
        let ec_dir = self.ec_dir(bucket, key);
        if Self::is_chunked_path(&ec_dir).await {
            let manifest = self.read_manifest(bucket, key).await?;
            if let Some(ref enc_meta) = meta.encryption {
                let dek = self.resolve_dek(enc_meta, customer_key)?;
                verify_sidecar_mac(&meta, &dek)?;
                let frame_size = enc_meta.chunk_size as usize;
                let plaintext_size = meta.size;
                let aad_builder = object_aad_builder(bucket, key, meta.version_id.as_deref());
                let mut ct_reader = VerifiedChunkReader::new(ec_dir, manifest);
                preflight_chunk_reader(&mut ct_reader)?;
                let mut decryptor = FrameDecryptor::new(
                    Box::pin(ct_reader),
                    &dek,
                    plaintext_size,
                    frame_size,
                    aad_builder,
                );
                preflight_frame_decryptor(&mut decryptor).await?;
                return Ok((Box::pin(decryptor), meta));
            }
            let mut reader = VerifiedChunkReader::new(ec_dir, manifest);
            preflight_chunk_reader(&mut reader)?;
            return Ok((Box::pin(reader), meta));
        }
        let obj_path = self.object_path(bucket, key);
        // Encrypted object — wrap in FrameDecryptor
        if let Some(ref enc_meta) = meta.encryption {
            let dek = self.resolve_dek(enc_meta, customer_key)?;
            verify_sidecar_mac(&meta, &dek)?;
            let chunk_size = enc_meta.chunk_size as usize;
            let plaintext_size = meta.size;
            let file = fs::File::open(&obj_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::NotFound(key.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            let aad_builder = object_aad_builder(bucket, key, meta.version_id.as_deref());
            let mut decryptor = FrameDecryptor::new(
                Box::pin(file),
                &dek,
                plaintext_size,
                chunk_size,
                aad_builder,
            );
            preflight_frame_decryptor(&mut decryptor).await?;
            return Ok((Box::pin(decryptor), meta));
        }
        if meta.size <= SMALL_OBJECT_THRESHOLD {
            let data = fs::read(&obj_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::NotFound(key.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            return Ok((Box::pin(std::io::Cursor::new(data)), meta));
        }
        let file = fs::File::open(&obj_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        let reader = BufReader::with_capacity(IO_BUFFER_SIZE, file);
        Ok((Box::pin(reader), meta))
    }

    pub async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        length: u64,
        customer_key: Option<[u8; 32]>,
    ) -> Result<(ByteStream, ObjectMeta), StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let meta = self.read_object_meta(bucket, key).await?;
        reject_sse_c_on_plaintext(&meta, customer_key.is_some())?;
        let ec_dir = self.ec_dir(bucket, key);
        if Self::is_chunked_path(&ec_dir).await {
            let manifest = self.read_manifest(bucket, key).await?;
            if let Some(ref enc_meta) = meta.encryption {
                let dek = self.resolve_dek(enc_meta, customer_key)?;
                verify_sidecar_mac(&meta, &dek)?;
                let frame_size = enc_meta.chunk_size as usize;
                let ct_offset = FrameDecryptor::ciphertext_offset(frame_size, offset);
                let ct_total = manifest.total_size;
                let ct_length = ct_total.saturating_sub(ct_offset);
                let aad_builder = object_aad_builder(bucket, key, meta.version_id.as_deref());
                let mut ct_reader =
                    VerifiedChunkReader::with_range(ec_dir, manifest, ct_offset, ct_length);
                preflight_chunk_reader(&mut ct_reader)?;
                let decryptor = FrameDecryptor::for_range(
                    Box::pin(ct_reader),
                    &dek,
                    meta.size,
                    frame_size,
                    offset,
                    length,
                    aad_builder,
                );
                return Ok((Box::pin(decryptor), meta));
            }
            let mut reader = VerifiedChunkReader::with_range(ec_dir, manifest, offset, length);
            preflight_chunk_reader(&mut reader)?;
            return Ok((Box::pin(reader), meta));
        }
        let obj_path = self.object_path(bucket, key);
        // Encrypted object — seek to frame boundary and wrap in ranged FrameDecryptor
        if let Some(ref enc_meta) = meta.encryption {
            let dek = self.resolve_dek(enc_meta, customer_key)?;
            verify_sidecar_mac(&meta, &dek)?;
            let chunk_size = enc_meta.chunk_size as usize;
            let ct_offset = FrameDecryptor::ciphertext_offset(chunk_size, offset);
            let mut file = fs::File::open(&obj_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::NotFound(key.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            file.seek(std::io::SeekFrom::Start(ct_offset))
                .await
                .map_err(StorageError::Io)?;
            let aad_builder = object_aad_builder(bucket, key, meta.version_id.as_deref());
            let decryptor = FrameDecryptor::for_range(
                Box::pin(file),
                &dek,
                meta.size,
                chunk_size,
                offset,
                length,
                aad_builder,
            );
            return Ok((Box::pin(decryptor), meta));
        }
        if length <= SMALL_OBJECT_THRESHOLD {
            let mut file = fs::File::open(&obj_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    StorageError::NotFound(key.to_string())
                } else {
                    StorageError::Io(e)
                }
            })?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(StorageError::Io)?;
            let mut data = vec![0u8; length as usize];
            file.read_exact(&mut data).await.map_err(StorageError::Io)?;
            return Ok((Box::pin(std::io::Cursor::new(data)), meta));
        }
        let mut file = fs::File::open(&obj_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(StorageError::Io)?;
        let limited = file.take(length);
        let reader = BufReader::with_capacity(IO_BUFFER_SIZE, limited);
        Ok((Box::pin(reader), meta))
    }

    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let meta = self.read_object_meta(bucket, key).await?;
        Ok(meta)
    }

    pub async fn get_object_tagging(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<std::collections::HashMap<String, String>, StorageError> {
        validate_key(key)?;
        let meta = self.read_object_meta(bucket, key).await?;
        Ok(meta.tags.unwrap_or_default())
    }

    pub async fn put_object_tagging(
        &self,
        bucket: &str,
        key: &str,
        tags: std::collections::HashMap<String, String>,
    ) -> Result<(), StorageError> {
        validate_key(key)?;
        let mut meta = self.read_object_meta(bucket, key).await?;
        meta.tags = if tags.is_empty() { None } else { Some(tags) };
        let json = serde_json::to_string_pretty(&meta)?;
        fs::write(self.meta_path(bucket, key), json).await?;
        Ok(())
    }

    pub async fn delete_object_tagging(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        validate_key(key)?;
        let mut meta = self.read_object_meta(bucket, key).await?;
        meta.tags = None;
        let json = serde_json::to_string_pretty(&meta)?;
        fs::write(self.meta_path(bucket, key), json).await?;
        Ok(())
    }

    pub async fn delete_object(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<DeleteResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;

        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        if versioned {
            let meta_path = self.meta_path(bucket, key);
            if fs::try_exists(&meta_path).await.unwrap_or(false) {
                let meta = self.read_object_meta(bucket, key).await?;
                self.ensure_can_delete_meta(&meta).await?;
            }
            return self.write_delete_marker(bucket, key).await;
        }

        let obj_path = self.object_path(bucket, key);
        let meta_path = self.meta_path(bucket, key);
        let ec_dir = self.ec_dir(bucket, key);

        if !fs::try_exists(&meta_path).await?
            && !fs::try_exists(&obj_path).await?
            && !fs::try_exists(&ec_dir).await?
        {
            return Ok(DeleteResult {
                version_id: None,
                is_delete_marker: false,
            });
        }
        if fs::try_exists(&meta_path).await? {
            let meta = self.read_object_meta(bucket, key).await?;
            self.ensure_can_delete_meta(&meta).await?;
        }
        remove_file_if_exists(&obj_path).await?;
        remove_file_if_exists(&meta_path).await?;
        remove_dir_all_if_exists(&ec_dir).await?;
        self.index_remove(bucket, key);

        // Clean up empty parent directories (but not the bucket dir itself)
        let bucket_dir = self.buckets_dir.join(bucket);
        let mut dir = obj_path.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            if d == bucket_dir {
                break;
            }
            match fs::remove_dir(&d).await {
                Ok(()) => {}
                Err(_) => break,
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }

        Ok(DeleteResult {
            version_id: None,
            is_delete_marker: false,
        })
    }
}
