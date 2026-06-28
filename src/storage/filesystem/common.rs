use super::*;

/// Validate that an object key does not contain path traversal components.
pub(super) fn validate_key(key: &str) -> Result<(), StorageError> {
    if key.is_empty() {
        return Err(StorageError::InvalidKey("Key must not be empty".into()));
    }
    if key.len() > 1024 {
        return Err(StorageError::InvalidKey(
            "Key must not exceed 1024 bytes".into(),
        ));
    }
    let path = Path::new(key);
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(StorageError::InvalidKey(
                    "Key must not contain '..' path components".into(),
                ));
            }
            Component::RootDir => {
                return Err(StorageError::InvalidKey(
                    "Key must not be an absolute path".into(),
                ));
            }
            Component::Normal(seg) => {
                let name = seg.to_string_lossy();
                if is_reserved_segment(&name) {
                    return Err(StorageError::InvalidKey(format!(
                        "Key segment '{}' collides with an internal storage name",
                        name
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Reject key path segments that collide with internal on-disk names
/// (sidecars, bucket metadata, internal dirs, EC dirs, temp files).
pub(super) fn is_reserved_segment(name: &str) -> bool {
    name.ends_with(".meta.json")
        || name.ends_with(".ec")
        || name == ".bucket.json"
        || name == ".uploads"
        || name == ".versions"
        || name == ".folder"
        || name.starts_with(".maxio-tmp-")
}

pub(super) fn validate_upload_id(upload_id: &str) -> Result<(), StorageError> {
    if upload_id.is_empty() {
        return Err(StorageError::UploadNotFound(upload_id.to_string()));
    }
    if upload_id.contains('/') || upload_id.contains('\\') || upload_id.contains("..") {
        return Err(StorageError::UploadNotFound(upload_id.to_string()));
    }
    Ok(())
}

/// Compute the 32-byte AAD for one frame of an object's ciphertext.
///
/// `aad = SHA-256(bucket || 0x00 || key || 0x00 || version_id || 0x00 || chunk_index_le_8B)`
///
/// Binds every GCM auth tag to object identity, detecting cross-object frame
/// swaps that would otherwise decrypt cleanly (same DEK + nonce + index).
pub(super) fn build_frame_aad(bucket: &str, key: &str, version_id: Option<&str>, chunk_index: u64) -> Vec<u8> {
    let mut hasher = <Sha256 as Digest>::new();
    hasher.update(bucket.as_bytes());
    hasher.update([0u8]);
    hasher.update(key.as_bytes());
    hasher.update([0u8]);
    hasher.update(version_id.unwrap_or("").as_bytes());
    hasher.update([0u8]);
    hasher.update(chunk_index.to_le_bytes());
    hasher.finalize().to_vec()
}

/// Build an `AadBuilder` closure for object frames. Captures the identifiers so
/// the frame writer/reader can produce per-chunk AAD on demand.
pub(super) fn object_aad_builder(bucket: &str, key: &str, version_id: Option<&str>) -> AadBuilder {
    let bucket = bucket.to_string();
    let key = key.to_string();
    let version_id = version_id.map(|v| v.to_string());
    Arc::new(move |chunk_index: u64| {
        build_frame_aad(&bucket, &key, version_id.as_deref(), chunk_index)
    })
}

/// Compute the 32-byte AAD for one frame of a multipart part's ciphertext.
///
/// `part_aad = SHA-256("PART" || 0x00 || upload_id || 0x00 || part_number_le_4B || 0x00 || chunk_index_le_8B)`
///
/// Binds part frames to the specific upload + part slot so they cannot be
/// shuffled between parts or other uploads without failing GCM authentication.
pub(super) fn build_part_aad(upload_id: &str, part_number: u32, chunk_index: u64) -> Vec<u8> {
    let mut hasher = <Sha256 as Digest>::new();
    hasher.update(b"PART");
    hasher.update([0u8]);
    hasher.update(upload_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(part_number.to_le_bytes());
    hasher.update([0u8]);
    hasher.update(chunk_index.to_le_bytes());
    hasher.finalize().to_vec()
}

/// Closure form of `build_part_aad` for the frame decryptor on `Complete`.
pub(super) fn part_aad_builder(upload_id: &str, part_number: u32) -> AadBuilder {
    let upload_id = upload_id.to_string();
    Arc::new(move |chunk_index: u64| build_part_aad(&upload_id, part_number, chunk_index))
}

/// Strip all mutable fields of `ObjectMeta` to produce the canonical input
/// that the sidecar MAC is computed over. Fields that MAY be edited after
/// initial write (tags, delete marker, `sidecar_mac` itself) are cleared so a
/// legitimate tag update does not invalidate the MAC.
pub(super) fn mac_input(meta: &ObjectMeta) -> ObjectMeta {
    let mut m = meta.clone();
    m.tags = None;
    m.is_delete_marker = false;
    if let Some(ref mut e) = m.encryption {
        e.sidecar_mac = String::new();
    }
    m
}

/// Recursively sort all JSON object keys so that `serde_json::to_vec` produces
/// a deterministic byte stream. `ObjectMeta` contains `HashMap` fields whose
/// iteration order is not stable, so a canonical representation is required.
pub(super) fn canonical_json_value(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonical_json_value(&m[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonical_json_value).collect()),
        _ => v.clone(),
    }
}

/// Compute the hex-encoded HMAC-SHA256 over `mac_input(meta)` keyed by the DEK.
pub(super) fn compute_sidecar_mac(dek: &[u8; 32], meta: &ObjectMeta) -> Result<String, StorageError> {
    let input = mac_input(meta);
    let value = serde_json::to_value(&input)?;
    let canonical = canonical_json_value(&value);
    let bytes = serde_json::to_vec(&canonical)?;
    let mut mac = <HmacSha256 as Mac>::new_from_slice(dek)
        .map_err(|_| StorageError::EncryptionError("hmac init failed".into()))?;
    mac.update(&bytes);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// Verify the stored `sidecar_mac` against a freshly computed MAC. Used by the
/// read path before any ciphertext is decrypted.
pub(super) fn verify_sidecar_mac(meta: &ObjectMeta, dek: &[u8; 32]) -> Result<(), StorageError> {
    let enc = meta
        .encryption
        .as_ref()
        .ok_or_else(|| StorageError::IntegrityError("object has no encryption metadata".into()))?;
    let expected = compute_sidecar_mac(dek, meta)?;
    if enc.sidecar_mac.is_empty() {
        return Err(StorageError::IntegrityError(
            "sidecar_mac missing — object may be tampered".into(),
        ));
    }
    if !constant_time_eq(expected.as_bytes(), enc.sidecar_mac.as_bytes()) {
        return Err(StorageError::IntegrityError(
            "sidecar_mac mismatch — object metadata has been tampered".into(),
        ));
    }
    Ok(())
}

/// Reject GET/HEAD requests that carry SSE-C headers but target an object that
/// was not encrypted. Matches AWS `InvalidRequest` behavior and prevents the
/// client from getting the false impression that SSE-C protected the response.
pub(super) fn reject_sse_c_on_plaintext(
    meta: &ObjectMeta,
    has_customer_key: bool,
) -> Result<(), StorageError> {
    if has_customer_key && meta.encryption.is_none() {
        return Err(StorageError::DecryptionError(
            "SSE-C headers supplied but object is not encrypted".into(),
        ));
    }
    Ok(())
}

pub(super) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub(super) async fn write_chunk_to_dir(
    dir: &Path,
    index: u32,
    data: &[u8],
) -> Result<ChunkInfo, StorageError> {
    write_chunk_file(&dir.join(format!("{:06}", index)), index, data).await
}

pub(super) async fn write_chunk_file(path: &Path, index: u32, data: &[u8]) -> Result<ChunkInfo, StorageError> {
    let sha256 = hex::encode(Sha256::digest(data));
    let mut file = fs::File::create(&path).await?;
    file.write_all(data).await?;
    file.flush().await?;
    Ok(ChunkInfo {
        index,
        size: data.len() as u64,
        sha256,
        kind: ChunkKind::Data,
    })
}

pub(super) fn temp_sibling_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".maxio-tmp-{}", uuid::Uuid::new_v4()))
}

pub(super) struct TempPathGuard {
    path: PathBuf,
    is_dir: bool,
    armed: bool,
}

impl TempPathGuard {
    pub(super) fn file(path: PathBuf) -> Self {
        Self {
            path,
            is_dir: false,
            armed: true,
        }
    }

    pub(super) fn dir(path: PathBuf) -> Self {
        Self {
            path,
            is_dir: true,
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempPathGuard {
    fn drop(&mut self) {
        if self.armed {
            if self.is_dir {
                let _ = std::fs::remove_dir_all(&self.path);
            } else {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

pub(super) async fn publish_temp_payload_and_meta(
    tmp_payload: &Path,
    final_payload: &Path,
    payload_is_dir: bool,
    tmp_meta: &Path,
    final_meta: &Path,
) -> Result<(), StorageError> {
    if let Some(parent) = final_payload.parent() {
        fs::create_dir_all(parent).await?;
    }
    if let Some(parent) = final_meta.parent() {
        fs::create_dir_all(parent).await?;
    }

    let payload_backup = backup_existing(final_payload).await?;
    let meta_backup = backup_existing(final_meta).await?;

    if let Err(e) = fs::rename(tmp_payload, final_payload).await {
        restore_backup(final_meta, &meta_backup, false).await;
        restore_backup(final_payload, &payload_backup, payload_is_dir).await;
        return Err(StorageError::Io(e));
    }

    if let Err(e) = fs::rename(tmp_meta, final_meta).await {
        remove_path_if_exists(final_payload, payload_is_dir).await;
        restore_backup(final_meta, &meta_backup, false).await;
        restore_backup(final_payload, &payload_backup, payload_is_dir).await;
        return Err(StorageError::Io(e));
    }

    cleanup_backup(&payload_backup, payload_is_dir).await;
    cleanup_backup(&meta_backup, false).await;
    Ok(())
}

pub(super) async fn backup_existing(path: &Path) -> Result<Option<PathBuf>, StorageError> {
    if !fs::try_exists(path).await? {
        return Ok(None);
    }
    let backup = temp_sibling_path(path);
    fs::rename(path, &backup).await?;
    Ok(Some(backup))
}

pub(super) async fn restore_backup(final_path: &Path, backup: &Option<PathBuf>, is_dir: bool) {
    if let Some(backup) = backup {
        remove_path_if_exists(final_path, is_dir).await;
        let _ = fs::rename(backup, final_path).await;
    }
}

pub(super) async fn cleanup_backup(backup: &Option<PathBuf>, is_dir: bool) {
    if let Some(backup) = backup {
        remove_path_if_exists(backup, is_dir).await;
    }
}

pub(super) async fn remove_path_if_exists(path: &Path, is_dir: bool) {
    if is_dir {
        let _ = fs::remove_dir_all(path).await;
    } else {
        let _ = fs::remove_file(path).await;
    }
}

/// Encrypt and write one frame: [nonce:12B][ciphertext||tag:16B]. The AAD
/// binds the frame to object identity (bucket/key/version/chunk_index).
pub(super) async fn write_encrypted_frame(
    writer: &mut BufWriter<fs::File>,
    cipher: &Aes256Gcm,
    nonce_prefix: &[u8],
    chunk_index: u64,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(), StorageError> {
    let nonce_bytes = make_frame_nonce(nonce_prefix, chunk_index)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| StorageError::EncryptionError("frame encryption failed".into()))?;
    writer.write_all(&nonce_bytes).await?;
    writer.write_all(&ciphertext).await?;
    Ok(())
}

/// Encrypt one frame and return the `[nonce || ciphertext || tag]` bytes. Used
/// by the EC+encryption write path, which buffers ciphertext in memory before
/// flushing chunk-sized slices to disk.
pub(super) fn encrypt_frame_to_vec(
    cipher: &Aes256Gcm,
    nonce_prefix: &[u8],
    chunk_index: u64,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, StorageError> {
    let nonce_bytes = make_frame_nonce(nonce_prefix, chunk_index)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| StorageError::EncryptionError("frame encryption failed".into()))?;
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub(super) fn make_frame_nonce(prefix: &[u8], chunk_index: u64) -> Result<[u8; 12], StorageError> {
    let mut nonce = [0u8; 12];
    match prefix.len() {
        4 => {
            nonce[..4].copy_from_slice(prefix);
            nonce[4..].copy_from_slice(&chunk_index.to_le_bytes());
        }
        8 => {
            if chunk_index > u32::MAX as u64 {
                return Err(StorageError::EncryptionError(
                    "object has too many encrypted frames for nonce format".into(),
                ));
            }
            nonce[..8].copy_from_slice(prefix);
            nonce[8..].copy_from_slice(&(chunk_index as u32).to_le_bytes());
        }
        _ => {
            return Err(StorageError::EncryptionError(
                "nonce_prefix must be 4 or 8 bytes".into(),
            ));
        }
    }
    Ok(nonce)
}

pub(super) async fn remove_file_if_exists(path: &Path) -> Result<(), StorageError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StorageError::Io(e)),
    }
}

pub(super) async fn remove_dir_all_if_exists(path: &Path) -> Result<(), StorageError> {
    match fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StorageError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_key;
    use crate::storage::StorageError;

    #[test]
    fn validate_key_rejects_traversal_and_reserved_segments() {
        assert!(validate_key("safe/key.txt").is_ok());
        assert!(matches!(
            validate_key("../etc/passwd"),
            Err(StorageError::InvalidKey(_))
        ));
        assert!(matches!(
            validate_key("foo/.meta.json"),
            Err(StorageError::InvalidKey(_))
        ));
        assert!(matches!(validate_key(""), Err(StorageError::InvalidKey(_))));
    }
}
