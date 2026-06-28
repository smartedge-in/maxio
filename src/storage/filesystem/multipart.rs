use super::*;

impl FilesystemStorage {
    pub(super) async fn complete_multipart_chunked(
        &self,
        bucket: &str,
        upload_id: &str,
        upload_meta: &MultipartUploadMeta,
        selected: &[PartMeta],
    ) -> Result<PutResult, StorageError> {
        let key = &upload_meta.key;
        let ec_dir = self.ec_dir(bucket, key);
        let tmp_ec_dir = temp_sibling_path(&ec_dir);
        let mut tmp_ec_guard = TempPathGuard::dir(tmp_ec_dir.clone());
        if let Some(parent) = ec_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::create_dir_all(&tmp_ec_dir).await?;
        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        let mut total_size = 0u64;
        let mut etag_hasher = Md5::new();
        let mut chunks: Vec<ChunkInfo> = Vec::new();
        let mut chunk_index: u32 = 0;
        let mut chunk_buf = Vec::with_capacity(self.chunk_size as usize);

        let mut buf = vec![0u8; IO_BUFFER_SIZE];
        for part in selected {
            let mut part_file =
                fs::File::open(self.part_path(bucket, upload_id, part.part_number)).await?;
            loop {
                let n = part_file.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                total_size += n as u64;
                chunk_buf.extend_from_slice(&buf[..n]);

                while chunk_buf.len() >= self.chunk_size as usize {
                    let chunk_data: Vec<u8> = chunk_buf.drain(..self.chunk_size as usize).collect();
                    let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_data).await?;
                    chunks.push(ci);
                    chunk_index += 1;
                }
            }

            let raw_md5 = hex::decode(part.etag.trim_matches('"'))
                .map_err(|_| StorageError::InvalidKey("invalid part etag".into()))?;
            etag_hasher.update(raw_md5);
        }

        // Flush remaining
        if !chunk_buf.is_empty() {
            let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_buf).await?;
            chunks.push(ci);
        }

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
        fs::write(
            tmp_ec_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )
        .await?;

        let etag = format!(
            "\"{}-{}\"",
            hex::encode(etag_hasher.finalize()),
            selected.len()
        );

        // Compute composite checksum if algorithm was specified
        let (checksum_algorithm, checksum_value) =
            if let Some(algo) = upload_meta.checksum_algorithm {
                let b64 = base64::engine::general_purpose::STANDARD;
                let mut raw_checksums = Vec::new();
                for part in selected {
                    if let Some(ref val) = part.checksum_value {
                        if let Ok(raw) = b64.decode(val) {
                            raw_checksums.extend_from_slice(&raw);
                        }
                    }
                }
                if !raw_checksums.is_empty() {
                    let mut composite_hasher = ChecksumHasher::new(algo);
                    composite_hasher.update(&raw_checksums);
                    let composite =
                        format!("{}-{}", composite_hasher.finalize_base64(), selected.len());
                    (Some(algo), Some(composite))
                } else {
                    (Some(algo), None)
                }
            } else {
                (None, None)
            };

        let part_sizes: Vec<u64> = selected.iter().map(|p| p.size).collect();
        let storage_format = if has_parity {
            "chunked-v2"
        } else {
            "chunked-v1"
        };
        let object_meta = ObjectMeta {
            key: key.to_string(),
            size: total_size,
            etag: etag.clone(),
            content_type: upload_meta.content_type.clone(),
            last_modified: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: Some(storage_format.to_string()),
            checksum_algorithm,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: Some(part_sizes),
            encryption: None,
        };

        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, serde_json::to_string_pretty(&object_meta)?).await?;
        publish_temp_payload_and_meta(&tmp_ec_dir, &ec_dir, true, &tmp_meta_path, &meta_path)
            .await?;
        tmp_ec_guard.disarm();
        tmp_meta_guard.disarm();
        if versioned {
            self.write_version_chunked(bucket, key, &object_meta)
                .await?;
        }
        fs::remove_dir_all(self.upload_dir(bucket, upload_id)).await?;

        Ok(PutResult {
            size: total_size,
            etag,
            version_id,
            checksum_algorithm,
            checksum_value,
        })
    }

    /// Encrypt-then-EC multipart completion. Reads each part with the
    /// upload-scoped DEK (per `upload_meta.encryption_spec`), re-encrypts the
    /// recombined stream under a fresh per-object DEK using 64 KiB frames,
    /// chunks the ciphertext into EC chunks, writes parity.
    pub(super) async fn complete_multipart_chunked_encrypted(
        &self,
        bucket: &str,
        upload_id: &str,
        upload_meta: &MultipartUploadMeta,
        selected: &[PartMeta],
        customer_key: Option<[u8; 32]>,
    ) -> Result<PutResult, StorageError> {
        let key = upload_meta.key.as_str();
        let ec_dir = self.ec_dir(bucket, key);
        let tmp_ec_dir = temp_sibling_path(&ec_dir);
        let mut tmp_ec_guard = TempPathGuard::dir(tmp_ec_dir.clone());
        if let Some(parent) = ec_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::create_dir_all(&tmp_ec_dir).await?;
        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        // Upload-scoped DEK used to decrypt each part on read.
        let upload_spec = upload_meta
            .encryption_spec
            .as_ref()
            .expect("complete_multipart_chunked_encrypted called without encryption_spec");
        let upload_dek = self.resolve_upload_dek(upload_spec, customer_key)?;

        // Fresh per-object encryption (distinct DEK from the upload DEK).
        let req = match upload_spec.mode {
            EncryptionMode::SseS3 => EncryptionRequest::sse_s3(),
            EncryptionMode::SseC => {
                let ck = customer_key.ok_or_else(|| {
                    StorageError::EncryptionError(
                        "SSE-C requires customer key on CompleteMultipartUpload".into(),
                    )
                })?;
                EncryptionRequest::sse_c(ck)
            }
        };
        let enc_meta = self
            .prepare_encryption(&req)
            .map_err(|e| StorageError::EncryptionError(e.to_string()))?;
        let dek = self
            .resolve_dek(&enc_meta, customer_key)
            .map_err(|e| StorageError::EncryptionError(e.to_string()))?;
        let b64 = base64::engine::general_purpose::STANDARD;
        let prefix_bytes = b64
            .decode(&enc_meta.nonce_prefix)
            .map_err(|_| StorageError::EncryptionError("invalid nonce_prefix".into()))?;
        let nonce_prefix = prefix_bytes;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));

        let mut total_plaintext: u64 = 0;
        let mut ct_size: u64 = 0;
        let mut etag_hasher = Md5::new();
        let mut chunks: Vec<ChunkInfo> = Vec::new();
        let mut chunk_index: u32 = 0;
        let mut frame_index: u64 = 0;
        let mut read_buf = vec![0u8; IO_BUFFER_SIZE];
        let mut frame_buf: Vec<u8> = Vec::with_capacity(FRAME_CHUNK_SIZE);
        let mut chunk_buf: Vec<u8> = Vec::with_capacity(self.chunk_size as usize);

        for part in selected {
            let part_path = self.part_path(bucket, upload_id, part.part_number);
            let mut part_stream: ByteStream = if part.encrypted {
                let file = fs::File::open(&part_path).await?;
                let aad = part_aad_builder(upload_id, part.part_number);
                Box::pin(FrameDecryptor::new(
                    Box::pin(file),
                    &upload_dek,
                    part.size,
                    FRAME_CHUNK_SIZE,
                    aad,
                ))
            } else {
                Box::pin(fs::File::open(&part_path).await?)
            };

            loop {
                let n = part_stream.read(&mut read_buf).await?;
                if n == 0 {
                    break;
                }
                total_plaintext += n as u64;
                frame_buf.extend_from_slice(&read_buf[..n]);
                while frame_buf.len() >= FRAME_CHUNK_SIZE {
                    let frame_data: Vec<u8> = frame_buf.drain(..FRAME_CHUNK_SIZE).collect();
                    let aad = build_frame_aad(bucket, key, version_id.as_deref(), frame_index);
                    let ct = encrypt_frame_to_vec(
                        &cipher,
                        &nonce_prefix,
                        frame_index,
                        &frame_data,
                        &aad,
                    )?;
                    chunk_buf.extend_from_slice(&ct);
                    frame_index += 1;
                    while chunk_buf.len() >= self.chunk_size as usize {
                        let chunk_data: Vec<u8> =
                            chunk_buf.drain(..self.chunk_size as usize).collect();
                        ct_size += chunk_data.len() as u64;
                        let ci = write_chunk_to_dir(&tmp_ec_dir, chunk_index, &chunk_data).await?;
                        chunks.push(ci);
                        chunk_index += 1;
                    }
                }
            }

            let raw_md5 = hex::decode(part.etag.trim_matches('"'))
                .map_err(|_| StorageError::InvalidKey("invalid part etag".into()))?;
            etag_hasher.update(raw_md5);
        }

        // Flush trailing partial frame + any remaining chunk_buf bytes.
        if !frame_buf.is_empty() {
            let aad = build_frame_aad(bucket, key, version_id.as_deref(), frame_index);
            let ct = encrypt_frame_to_vec(&cipher, &nonce_prefix, frame_index, &frame_buf, &aad)?;
            chunk_buf.extend_from_slice(&ct);
            frame_buf.clear();
        }
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
            plaintext_size: Some(total_plaintext),
        };
        fs::write(
            tmp_ec_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )
        .await?;

        let etag = format!(
            "\"{}-{}\"",
            hex::encode(etag_hasher.finalize()),
            selected.len()
        );

        let (checksum_algorithm, checksum_value) =
            if let Some(algo) = upload_meta.checksum_algorithm {
                let mut raw_checksums = Vec::new();
                for part in selected {
                    if let Some(ref val) = part.checksum_value {
                        if let Ok(raw) = b64.decode(val) {
                            raw_checksums.extend_from_slice(&raw);
                        }
                    }
                }
                if !raw_checksums.is_empty() {
                    let mut composite_hasher = ChecksumHasher::new(algo);
                    composite_hasher.update(&raw_checksums);
                    let composite =
                        format!("{}-{}", composite_hasher.finalize_base64(), selected.len());
                    (Some(algo), Some(composite))
                } else {
                    (Some(algo), None)
                }
            } else {
                (None, None)
            };

        let part_sizes: Vec<u64> = selected.iter().map(|p| p.size).collect();
        let storage_format = if has_parity {
            "chunked-v2"
        } else {
            "chunked-v1"
        };
        let mut object_meta = ObjectMeta {
            key: key.to_string(),
            size: total_plaintext,
            etag: etag.clone(),
            content_type: upload_meta.content_type.clone(),
            last_modified: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: Some(storage_format.to_string()),
            checksum_algorithm,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: Some(part_sizes),
            encryption: Some(enc_meta),
        };
        object_meta.encryption.as_mut().unwrap().sidecar_mac = String::new();
        let mac = compute_sidecar_mac(&dek, &object_meta)?;
        object_meta.encryption.as_mut().unwrap().sidecar_mac = mac;

        let meta_path = self.meta_path(bucket, key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, serde_json::to_string_pretty(&object_meta)?).await?;
        publish_temp_payload_and_meta(&tmp_ec_dir, &ec_dir, true, &tmp_meta_path, &meta_path)
            .await?;
        tmp_ec_guard.disarm();
        tmp_meta_guard.disarm();
        if versioned {
            self.write_version_chunked(bucket, key, &object_meta)
                .await?;
        }
        fs::remove_dir_all(self.upload_dir(bucket, upload_id)).await?;

        Ok(PutResult {
            size: total_plaintext,
            etag,
            version_id,
            checksum_algorithm,
            checksum_value,
        })
    }

    pub(super) async fn put_folder_marker(&self, bucket: &str, key: &str) -> Result<PutResult, StorageError> {
        let folder_dir = self
            .buckets_dir
            .join(bucket)
            .join(key.trim_end_matches('/'));
        fs::create_dir_all(&folder_dir).await?;

        let marker_path = folder_dir.join(".folder");
        fs::write(&marker_path, b"").await?;

        let etag = "\"d41d8cd98f00b204e9800998ecf8427e\"".to_string();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        let meta = ObjectMeta {
            key: key.to_string(),
            size: 0,
            etag: etag.clone(),
            content_type: "application/x-directory".to_string(),
            last_modified: now,
            version_id: None,
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm: None,
            checksum_value: None,
            tags: None,
            part_sizes: None,
            encryption: None,
        };

        let meta_path = folder_dir.join(".folder.meta.json");
        let json = serde_json::to_string_pretty(&meta)?;
        fs::write(&meta_path, json).await?;

        Ok(PutResult {
            size: 0,
            etag,
            version_id: None,
            checksum_algorithm: None,
            checksum_value: None,
        })
    }


    pub async fn create_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        checksum_algorithm: Option<ChecksumAlgorithm>,
        encryption_spec: Option<UploadEncryptionSpec>,
    ) -> Result<MultipartUploadMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let upload_id = uuid::Uuid::new_v4().to_string();
        let upload_dir = self.upload_dir(bucket, &upload_id);
        fs::create_dir_all(&upload_dir).await?;

        // Augment the spec with an upload-scoped DEK so every UploadPart can
        // encrypt its bytes before they touch disk. SSE-C reuses the customer
        // key directly (never persisted); SSE-S3 wraps a fresh random DEK with
        // the active master.
        let encryption_spec = if let Some(mut spec) = encryption_spec {
            let b64 = base64::engine::general_purpose::STANDARD;
            let prefix = Keyring::generate_nonce_prefix8();
            spec.upload_nonce_prefix = b64.encode(prefix);
            if matches!(spec.mode, EncryptionMode::SseS3) {
                let dek = Keyring::generate_dek();
                let kid = self.keyring.active_id().to_string();
                let (wrapped, wrap_nonce) = self
                    .keyring
                    .wrap_dek(&kid, &dek)
                    .map_err(|e| StorageError::EncryptionError(e.to_string()))?;
                spec.upload_dek_wrapped = Some(b64.encode(&wrapped));
                spec.upload_dek_wrap_nonce = Some(b64.encode(wrap_nonce));
                spec.upload_dek_key_id = Some(kid);
            }
            Some(spec)
        } else {
            None
        };

        let meta = MultipartUploadMeta {
            upload_id: upload_id.clone(),
            bucket: bucket.to_string(),
            key: key.to_string(),
            content_type: content_type.to_string(),
            initiated: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            checksum_algorithm,
            encryption_spec,
        };

        let meta_json = serde_json::to_string_pretty(&meta)?;
        fs::write(self.upload_meta_path(bucket, &upload_id), meta_json).await?;
        Ok(meta)
    }

    pub async fn upload_part(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
        body: ByteStream,
        checksum: Option<(ChecksumAlgorithm, Option<String>)>,
        customer_key: Option<[u8; 32]>,
        declared_size: Option<u64>,
    ) -> Result<PartMeta, StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        self.check_upload_start(declared_size)?;
        let mut body = self.wrap_upload_reader(body);
        if part_number == 0 || part_number > 10_000 {
            return Err(StorageError::InvalidKey(
                "part number must be 1..=10000".into(),
            ));
        }
        let upload_dir = self.upload_dir(bucket, upload_id);
        if !fs::try_exists(&upload_dir).await? {
            return Err(StorageError::UploadNotFound(upload_id.to_string()));
        }

        let upload_meta = self.read_upload_meta(bucket, upload_id).await?;
        let (cipher_opt, nonce_prefix) = if let Some(ref spec) = upload_meta.encryption_spec {
            let b64 = base64::engine::general_purpose::STANDARD;
            let dek = self.resolve_upload_dek(spec, customer_key)?;
            let prefix_bytes = b64
                .decode(&spec.upload_nonce_prefix)
                .map_err(|_| StorageError::EncryptionError("invalid upload_nonce_prefix".into()))?;
            if prefix_bytes.len() != 4 && prefix_bytes.len() != 8 {
                return Err(StorageError::EncryptionError(
                    "upload_nonce_prefix must be 4 or 8 bytes".into(),
                ));
            }
            let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
            (Some(cipher), prefix_bytes)
        } else {
            (None, Vec::new())
        };

        let part_path = self.part_path(bucket, upload_id, part_number);
        let file = fs::File::create(&part_path).await?;
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
                if let Some(ref cipher) = cipher_opt {
                    if !frame_buf.is_empty() {
                        let aad = build_part_aad(upload_id, part_number, chunk_index);
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
                    let aad = build_part_aad(upload_id, part_number, chunk_index);
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

        // Validate and compute checksum
        let (checksum_algorithm, checksum_value) = if let Some((algo, expected)) = checksum {
            let computed = checksum_hasher.unwrap().finalize_base64();
            if let Some(expected_val) = expected {
                if computed != expected_val {
                    let _ = fs::remove_file(&part_path).await;
                    return Err(StorageError::ChecksumMismatch(format!(
                        "expected {}, got {}",
                        expected_val, computed
                    )));
                }
            }
            (Some(algo), Some(computed))
        } else {
            (None, None)
        };

        let encrypted = cipher_opt.is_some();
        let ciphertext_size = if encrypted {
            Some(fs::metadata(&part_path).await?.len())
        } else {
            None
        };

        let etag = format!("\"{}\"", hex::encode(hasher.finalize()));
        let meta = PartMeta {
            part_number,
            etag,
            size,
            last_modified: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            checksum_algorithm,
            checksum_value,
            encrypted,
            ciphertext_size,
        };
        if let Err(e) = fs::write(
            self.part_meta_path(bucket, upload_id, part_number),
            serde_json::to_string_pretty(&meta)?,
        )
        .await
        {
            // Clean up orphaned part file on metadata write failure
            let _ = fs::remove_file(&part_path).await;
            return Err(e.into());
        }
        Ok(meta)
    }

    pub async fn complete_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
        parts: &[(u32, String)],
        customer_key: Option<[u8; 32]>,
    ) -> Result<PutResult, StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        if parts.is_empty() {
            return Err(StorageError::InvalidKey(
                "at least one part is required to complete upload".into(),
            ));
        }

        let upload_meta = self.read_upload_meta(bucket, upload_id).await?;
        let mut selected = Vec::with_capacity(parts.len());
        for (idx, (part_number, requested_etag)) in parts.iter().enumerate() {
            let meta = self.read_part_meta(bucket, upload_id, *part_number).await?;
            if meta.etag != *requested_etag {
                return Err(StorageError::InvalidKey(format!(
                    "etag mismatch for part {}",
                    part_number
                )));
            }
            if idx + 1 < parts.len() && meta.size < 5 * 1024 * 1024 {
                return Err(StorageError::InvalidKey("part too small".into()));
            }
            selected.push(meta);
        }

        let total_size: u64 = selected.iter().map(|p| p.size).sum();
        self.quota.check_object_size(total_size)?;
        self.quota.check_disk_reserve(&self.data_root)?;

        if self.erasure_coding {
            if upload_meta.encryption_spec.is_some() {
                // SSE-C key continuity + per-part `encrypted` flag checks
                // belong with the encrypted multipart path even under EC.
                if let Some(ref spec) = upload_meta.encryption_spec {
                    if matches!(spec.mode, EncryptionMode::SseC) {
                        let ck = customer_key.ok_or_else(|| {
                            StorageError::EncryptionError(
                                "SSE-C requires customer key on CompleteMultipartUpload".into(),
                            )
                        })?;
                        if let Some(ref stored) = spec.customer_key_md5 {
                            let b64 = base64::engine::general_purpose::STANDARD;
                            let provided = b64.encode(Md5::digest(ck));
                            if provided != *stored {
                                return Err(StorageError::EncryptionError(
                                    "SSE-C key changed between Create and Complete".into(),
                                ));
                            }
                        }
                    }
                }
                let upload_is_encrypted = true;
                for part in &selected {
                    if part.encrypted != upload_is_encrypted {
                        return Err(StorageError::IntegrityError(format!(
                            "part {} encryption flag ({}) disagrees with upload spec ({}) — part meta may be tampered",
                            part.part_number, part.encrypted, upload_is_encrypted,
                        )));
                    }
                }
                return self
                    .complete_multipart_chunked_encrypted(
                        bucket,
                        upload_id,
                        &upload_meta,
                        &selected,
                        customer_key,
                    )
                    .await;
            }
            return self
                .complete_multipart_chunked(bucket, upload_id, &upload_meta, &selected)
                .await;
        }

        // If the upload was encrypted, verify the SSE-C key (if any) matches
        // the one declared at CreateMultipartUpload. This closes the "init with
        // key A, complete with key B" gap — without this check the final
        // object would be encrypted with the wrong key and the parts
        // (encrypted under the Create-time key) could not be decrypted
        // consistently anyway.
        if let Some(ref spec) = upload_meta.encryption_spec {
            if matches!(spec.mode, EncryptionMode::SseC) {
                let ck = customer_key.ok_or_else(|| {
                    StorageError::EncryptionError(
                        "SSE-C requires customer key on CompleteMultipartUpload".into(),
                    )
                })?;
                if let Some(ref stored) = spec.customer_key_md5 {
                    let b64 = base64::engine::general_purpose::STANDARD;
                    let provided = b64.encode(Md5::digest(ck));
                    if provided != *stored {
                        return Err(StorageError::EncryptionError(
                            "SSE-C key changed between Create and Complete".into(),
                        ));
                    }
                }
            }
        }

        // Cross-check each part's `encrypted` flag against the upload spec so
        // a flipped `encrypted: true → false` cannot coerce the server into
        // reading ciphertext as plaintext during concat. Both modes (always-on
        // or always-off) are enforced.
        let upload_is_encrypted = upload_meta.encryption_spec.is_some();
        for part in &selected {
            if part.encrypted != upload_is_encrypted {
                return Err(StorageError::IntegrityError(format!(
                    "part {} encryption flag ({}) disagrees with upload spec ({}) — part meta may be tampered",
                    part.part_number, part.encrypted, upload_is_encrypted,
                )));
            }
        }

        // Upload-scoped DEK used to decrypt every encrypted part on the way in.
        let upload_dek_opt: Option<[u8; 32]> = if let Some(ref spec) = upload_meta.encryption_spec {
            Some(self.resolve_upload_dek(spec, customer_key)?)
        } else {
            None
        };

        // Final object encryption (fresh DEK, distinct from the upload DEK).
        let enc_meta_opt: Option<EncryptionMeta> =
            if let Some(ref spec) = upload_meta.encryption_spec {
                let req = match spec.mode {
                    EncryptionMode::SseS3 => EncryptionRequest::sse_s3(),
                    EncryptionMode::SseC => {
                        let ck = customer_key.ok_or_else(|| {
                            StorageError::EncryptionError(
                                "SSE-C requires customer key on CompleteMultipartUpload".into(),
                            )
                        })?;
                        EncryptionRequest::sse_c(ck)
                    }
                };
                Some(
                    self.prepare_encryption(&req)
                        .map_err(|e| StorageError::EncryptionError(e.to_string()))?,
                )
            } else {
                None
            };

        let (cipher_opt, nonce_prefix, dek_opt) = if let Some(ref em) = enc_meta_opt {
            let dek = self.resolve_dek(em, customer_key)?;
            let b64 = base64::engine::general_purpose::STANDARD;
            let prefix_bytes = b64
                .decode(&em.nonce_prefix)
                .map_err(|_| StorageError::EncryptionError("invalid nonce_prefix".into()))?;
            let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
            (Some(cipher), prefix_bytes, Some(dek))
        } else {
            (None, Vec::new(), None)
        };

        let versioned = self.is_versioned(bucket).await.unwrap_or(false);
        let version_id = if versioned {
            Some(Self::generate_version_id())
        } else {
            None
        };

        let obj_path = self.object_path(bucket, &upload_meta.key);
        if let Some(parent) = obj_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_obj_path = temp_sibling_path(&obj_path);
        let mut tmp_obj_guard = TempPathGuard::file(tmp_obj_path.clone());
        let out = fs::File::create(&tmp_obj_path).await?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, out);
        let mut total_size = 0u64;
        let mut etag_hasher = Md5::new();
        let mut buf = vec![0u8; IO_BUFFER_SIZE];
        let mut frame_buf: Vec<u8> = Vec::with_capacity(FRAME_CHUNK_SIZE);
        let mut chunk_index: u64 = 0;
        let bucket_for_aad = bucket;
        let key_for_aad = upload_meta.key.as_str();

        for part in &selected {
            let part_path = self.part_path(bucket, upload_id, part.part_number);
            let mut part_stream: ByteStream = if part.encrypted {
                let dek = upload_dek_opt.as_ref().ok_or_else(|| {
                    StorageError::EncryptionError(
                        "encrypted part but upload spec has no DEK".into(),
                    )
                })?;
                let file = fs::File::open(&part_path).await?;
                let aad = part_aad_builder(upload_id, part.part_number);
                Box::pin(FrameDecryptor::new(
                    Box::pin(file),
                    dek,
                    part.size,
                    FRAME_CHUNK_SIZE,
                    aad,
                ))
            } else {
                Box::pin(fs::File::open(&part_path).await?)
            };
            loop {
                let n = part_stream.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                total_size += n as u64;
                if let Some(ref cipher) = cipher_opt {
                    frame_buf.extend_from_slice(&buf[..n]);
                    while frame_buf.len() >= FRAME_CHUNK_SIZE {
                        let frame_data: Vec<u8> = frame_buf.drain(..FRAME_CHUNK_SIZE).collect();
                        let aad = build_frame_aad(
                            bucket_for_aad,
                            key_for_aad,
                            version_id.as_deref(),
                            chunk_index,
                        );
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

            let raw_md5 = hex::decode(part.etag.trim_matches('"'))
                .map_err(|_| StorageError::InvalidKey("invalid part etag".into()))?;
            etag_hasher.update(raw_md5);
        }
        // Flush trailing partial frame
        if let Some(ref cipher) = cipher_opt {
            if !frame_buf.is_empty() {
                let aad = build_frame_aad(
                    bucket_for_aad,
                    key_for_aad,
                    version_id.as_deref(),
                    chunk_index,
                );
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
        }
        writer.flush().await?;

        let etag = format!(
            "\"{}-{}\"",
            hex::encode(etag_hasher.finalize()),
            selected.len()
        );

        // Compute composite checksum if algorithm was specified
        let (checksum_algorithm, checksum_value) =
            if let Some(algo) = upload_meta.checksum_algorithm {
                let b64 = base64::engine::general_purpose::STANDARD;
                let mut raw_checksums = Vec::new();
                for part in &selected {
                    if let Some(ref val) = part.checksum_value {
                        if let Ok(raw) = b64.decode(val) {
                            raw_checksums.extend_from_slice(&raw);
                        }
                    }
                }
                if !raw_checksums.is_empty() {
                    let mut composite_hasher = ChecksumHasher::new(algo);
                    composite_hasher.update(&raw_checksums);
                    let composite =
                        format!("{}-{}", composite_hasher.finalize_base64(), selected.len());
                    (Some(algo), Some(composite))
                } else {
                    (Some(algo), None)
                }
            } else {
                (None, None)
            };

        let part_sizes: Vec<u64> = selected.iter().map(|p| p.size).collect();
        let mut object_meta = ObjectMeta {
            key: upload_meta.key.clone(),
            size: total_size,
            etag: etag.clone(),
            content_type: upload_meta.content_type,
            last_modified: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            version_id: version_id.clone(),
            is_delete_marker: false,
            storage_format: None,
            checksum_algorithm,
            checksum_value: checksum_value.clone(),
            tags: None,
            part_sizes: Some(part_sizes),
            encryption: enc_meta_opt,
        };
        if let (Some(dek), Some(em)) = (dek_opt.as_ref(), object_meta.encryption.as_mut()) {
            em.sidecar_mac = String::new();
            let mac = compute_sidecar_mac(dek, &object_meta)?;
            object_meta.encryption.as_mut().unwrap().sidecar_mac = mac;
        }
        let meta_path = self.meta_path(bucket, &upload_meta.key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_meta_path = temp_sibling_path(&meta_path);
        let mut tmp_meta_guard = TempPathGuard::file(tmp_meta_path.clone());
        fs::write(&tmp_meta_path, serde_json::to_string_pretty(&object_meta)?).await?;
        publish_temp_payload_and_meta(&tmp_obj_path, &obj_path, false, &tmp_meta_path, &meta_path)
            .await?;
        tmp_obj_guard.disarm();
        tmp_meta_guard.disarm();
        if versioned {
            self.write_version(bucket, &upload_meta.key, &object_meta, &obj_path)
                .await?;
        }
        fs::remove_dir_all(self.upload_dir(bucket, upload_id)).await?;

        Ok(PutResult {
            size: total_size,
            etag,
            version_id,
            checksum_algorithm,
            checksum_value,
        })
    }

    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(), StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        let upload_dir = self.upload_dir(bucket, upload_id);
        if !fs::try_exists(&upload_dir).await? {
            return Err(StorageError::UploadNotFound(upload_id.to_string()));
        }
        fs::remove_dir_all(upload_dir).await?;
        Ok(())
    }

    pub async fn list_parts(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<(MultipartUploadMeta, Vec<PartMeta>), StorageError> {
        validate_bucket_name(bucket)?;
        validate_upload_id(upload_id)?;
        let meta = self.read_upload_meta(bucket, upload_id).await?;
        let upload_dir = self.upload_dir(bucket, upload_id);
        let mut entries = fs::read_dir(&upload_dir).await?;
        let mut parts = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".meta.json") || name == ".meta.json" {
                continue;
            }
            let data = fs::read_to_string(entry.path()).await?;
            if let Ok(pm) = serde_json::from_str::<PartMeta>(&data) {
                parts.push(pm);
            }
        }
        parts.sort_by_key(|p| p.part_number);
        Ok((meta, parts))
    }

    pub async fn list_multipart_uploads(
        &self,
        bucket: &str,
    ) -> Result<Vec<MultipartUploadMeta>, StorageError> {
        validate_bucket_name(bucket)?;
        let uploads_dir = self.uploads_dir(bucket);
        if !fs::try_exists(&uploads_dir).await? {
            return Ok(Vec::new());
        }
        let mut entries = fs::read_dir(&uploads_dir).await?;
        let mut uploads = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }
            let upload_id = entry.file_name().to_string_lossy().to_string();
            if let Ok(meta) = self.read_upload_meta(bucket, &upload_id).await {
                uploads.push(meta);
            }
        }
        uploads.sort_by(|a, b| a.initiated.cmp(&b.initiated));
        Ok(uploads)
    }

    pub(super) async fn read_upload_meta(
        &self,
        bucket: &str,
        upload_id: &str,
    ) -> Result<MultipartUploadMeta, StorageError> {
        let path = self.upload_meta_path(bucket, upload_id);
        let data = fs::read_to_string(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::UploadNotFound(upload_id.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    pub(super) async fn read_part_meta(
        &self,
        bucket: &str,
        upload_id: &str,
        part_number: u32,
    ) -> Result<PartMeta, StorageError> {
        let path = self.part_meta_path(bucket, upload_id, part_number);
        let data = fs::read_to_string(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::InvalidKey(format!("missing part {}", part_number))
            } else {
                StorageError::Io(e)
            }
        })?;
        Ok(serde_json::from_str(&data)?)
    }

    // --- Versioning ---

}
