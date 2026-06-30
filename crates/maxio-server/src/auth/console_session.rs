//! Console cookie session tokens (credential-bound, P3-29).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::credentials::{CredentialEntry, CredentialStore};
use crate::app_state::AppState;

type HmacSha256 = Hmac<Sha256>;

pub const TOKEN_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// Short fingerprint tying a token to a specific access/secret pair.
pub fn session_fingerprint(access_key: &str, secret_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(access_key.as_bytes());
    hasher.update(b":");
    hasher.update(secret_key.as_bytes());
    hex::encode(&hasher.finalize()[..4])
}

pub fn generate_session_token(access_key: &str, secret_key: &str, issued_at: i64) -> String {
    let issued_hex = format!("{:x}", issued_at);
    let fp = session_fingerprint(access_key, secret_key);
    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(format!("{access_key}:{issued_hex}:{fp}").as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{issued_hex}.{sig}.{fp}")
}

pub fn verify_session_token(token: &str, access_key: &str, secret_key: &str) -> bool {
    let mut parts = token.split('.');
    let Some(issued_hex) = parts.next() else {
        return false;
    };
    let Some(signature) = parts.next() else {
        return false;
    };
    let Some(fp) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    let current_fp = session_fingerprint(access_key, secret_key);
    if !constant_time_eq(fp.as_bytes(), current_fp.as_bytes()) {
        return false;
    }

    let Ok(issued_at) = i64::from_str_radix(issued_hex, 16) else {
        return false;
    };

    let now = chrono::Utc::now().timestamp();
    if now - issued_at > TOKEN_MAX_AGE_SECS || issued_at > now + 60 {
        return false;
    }

    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(format!("{access_key}:{issued_hex}:{fp}").as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    constant_time_eq(signature.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

impl CredentialStore {
    /// Resolve the credential that minted a legacy console session token.
    pub fn lookup_by_session_fingerprint(&self, fp: &str) -> Option<&CredentialEntry> {
        self.list_access_keys()
            .into_iter()
            .filter_map(|key| self.lookup(key))
            .find(|cred| session_fingerprint(&cred.access_key, &cred.secret_key) == fp)
    }
}

/// Authenticated console access key (cookie session or Keycloak SSO).
#[derive(Clone, Debug)]
pub struct ConsolePrincipal {
    pub access_key: String,
}

/// Verify a legacy (non-JWT) console cookie and return the bound access key.
pub fn resolve_legacy_console_access_key(
    credentials: &CredentialStore,
    token: &str,
) -> Option<String> {
    let fp = token.split('.').nth(2)?;
    let cred = credentials.lookup_by_session_fingerprint(fp)?;
    if verify_session_token(token, &cred.access_key, &cred.secret_key) {
        Some(cred.access_key.clone())
    } else {
        None
    }
}

/// Resolve console principal from session cookie value.
pub async fn resolve_console_principal(state: &AppState, token: &str) -> Option<ConsolePrincipal> {
    if crate::auth::keycloak::is_legacy_console_session(token) {
        let access_key = resolve_legacy_console_access_key(&state.credentials, token)?;
        return Some(ConsolePrincipal { access_key });
    }
    if let Some(keycloak) = &state.keycloak {
        if keycloak.validate_access_token(token).await.is_ok() {
            return Some(ConsolePrincipal {
                access_key: state.config.access_key.clone(),
            });
        }
    }
    None
}

/// Extract bucket name from console API paths (`/buckets/{name}/...`).
pub fn bucket_from_console_path(path: &str) -> Option<String> {
    let trimmed = path.trim_start_matches('/');
    let mut parts = trimmed.split('/');
    if parts.next()? != "buckets" {
        return None;
    }
    let bucket = parts.next()?;
    if bucket.is_empty() {
        return None;
    }
    Some(bucket.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_token_binds_to_credential_not_bootstrap() {
        let store = CredentialStore::from_single("tenant-a-user", "secret-a");
        let now = chrono::Utc::now().timestamp();
        let token = generate_session_token("tenant-a-user", "secret-a", now);
        assert!(resolve_legacy_console_access_key(&store, &token).is_some());
        assert_eq!(
            resolve_legacy_console_access_key(&store, &token).as_deref(),
            Some("tenant-a-user")
        );
        // Token minted for tenant-a must not verify as bootstrap admin.
        assert!(!verify_session_token(&token, "admin", "admin-secret"));
    }

    #[test]
    fn parses_bucket_from_console_paths() {
        assert_eq!(
            bucket_from_console_path("/buckets/my-bucket/objects"),
            Some("my-bucket".into())
        );
        assert_eq!(bucket_from_console_path("/buckets"), None);
        assert_eq!(bucket_from_console_path("/auth/login"), None);
    }

    #[test]
    fn fingerprint_lookup_finds_correct_credential() {
        let store = CredentialStore::from_single("tenant-a-user", "secret-a");
        let fp = session_fingerprint("tenant-a-user", "secret-a");
        let cred = store.lookup_by_session_fingerprint(&fp).unwrap();
        assert_eq!(cred.access_key, "tenant-a-user");
    }
}
