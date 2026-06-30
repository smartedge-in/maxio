//! Pluggable KMS backend for SSE-KMS (`aws:kms`) object encryption.

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use base64::Engine;
use rand::RngExt;
use std::sync::Arc;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
pub const DEFAULT_LOCAL_KMS_KEY_ID: &str = "maxio-local-kms";

#[derive(Debug, thiserror::Error)]
pub enum KmsError {
    #[error("KMS not configured: set MAXIO_KMS_MASTER_KEY")]
    NotConfigured,
    #[error("invalid KMS master key: {0}")]
    InvalidMasterKey(String),
    #[error("KMS operation failed: {0}")]
    OperationFailed(String),
}

#[derive(Debug, Clone)]
pub struct GeneratedDataKey {
    pub plaintext: [u8; 32],
    pub ciphertext: Vec<u8>,
    pub wrap_nonce: [u8; 12],
    pub kms_key_id: String,
}

pub trait KmsBackend: Send + Sync {
    fn generate_data_key(&self, key_id: Option<&str>) -> Result<GeneratedDataKey, KmsError>;
    fn decrypt_data_key(
        &self,
        kms_key_id: &str,
        ciphertext: &[u8],
        nonce: &[u8; 12],
    ) -> Result<[u8; 32], KmsError>;
}

pub struct LocalKmsBackend {
    master_key: [u8; 32],
    default_key_id: String,
}

impl LocalKmsBackend {
    pub fn from_master_key_b64(b64_key: &str) -> Result<Self, KmsError> {
        let raw = B64
            .decode(b64_key.trim())
            .map_err(|_| KmsError::InvalidMasterKey("must be base64".into()))?;
        if raw.len() != 32 {
            return Err(KmsError::InvalidMasterKey(
                "decoded key must be exactly 32 bytes".into(),
            ));
        }
        let mut master_key = [0u8; 32];
        master_key.copy_from_slice(&raw);
        Ok(Self {
            master_key,
            default_key_id: DEFAULT_LOCAL_KMS_KEY_ID.to_string(),
        })
    }
}

impl KmsBackend for LocalKmsBackend {
    fn generate_data_key(&self, key_id: Option<&str>) -> Result<GeneratedDataKey, KmsError> {
        let kms_key_id = key_id
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.default_key_id)
            .to_string();
        let mut plaintext = [0u8; 32];
        rand::rng().fill(&mut plaintext[..]);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.master_key));
        let mut wrap_nonce = [0u8; 12];
        rand::rng().fill(&mut wrap_nonce[..]);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&wrap_nonce),
                Payload {
                    msg: plaintext.as_slice(),
                    aad: kms_key_id.as_bytes(),
                },
            )
            .map_err(|_| KmsError::OperationFailed("DEK wrap failed".into()))?;
        Ok(GeneratedDataKey {
            plaintext,
            ciphertext,
            wrap_nonce,
            kms_key_id,
        })
    }
    fn decrypt_data_key(
        &self,
        kms_key_id: &str,
        ciphertext: &[u8],
        nonce: &[u8; 12],
    ) -> Result<[u8; 32], KmsError> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.master_key));
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: kms_key_id.as_bytes(),
                },
            )
            .map_err(|_| KmsError::OperationFailed("DEK unwrap failed".into()))?;
        if plaintext.len() != 32 {
            return Err(KmsError::OperationFailed(format!(
                "unwrapped DEK length {} != 32",
                plaintext.len()
            )));
        }
        let mut dek = [0u8; 32];
        dek.copy_from_slice(&plaintext);
        Ok(dek)
    }
}

pub fn load_from_env() -> Result<Option<Arc<dyn KmsBackend>>, KmsError> {
    match std::env::var("MAXIO_KMS_MASTER_KEY") {
        Ok(v) if !v.trim().is_empty() => {
            Ok(Some(Arc::new(LocalKmsBackend::from_master_key_b64(&v)?)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_encrypt_decrypt() {
        let b64 = B64.encode([0xABu8; 32]);
        let kms = LocalKmsBackend::from_master_key_b64(&b64).unwrap();
        let g = kms.generate_data_key(Some("my-key")).unwrap();
        let dek = kms
            .decrypt_data_key(&g.kms_key_id, &g.ciphertext, &g.wrap_nonce)
            .unwrap();
        assert_eq!(dek, g.plaintext);
    }
}
