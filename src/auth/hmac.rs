use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 accepts keys of any length; failure here indicates a library bug.
pub fn hmac_sha256(key: &[u8]) -> HmacSha256 {
    HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts any key length")
}

pub fn hmac_sha256_digest(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = hmac_sha256(key);
    mac.update(data);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_deterministic() {
        let a = hmac_sha256_digest(b"key", b"data");
        let b = hmac_sha256_digest(b"key", b"data");
        assert_eq!(a, b);
        assert_ne!(a, hmac_sha256_digest(b"key", b"other"));
    }
}
