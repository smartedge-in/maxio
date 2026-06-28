//! Multi-credential store for S3 authentication (P1-10 phase 1).

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::fs;

const CREDENTIALS_FILE: &str = ".maxio-credentials.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    pub access_key: String,
    pub secret_key: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialsFile {
    #[serde(default)]
    credentials: Vec<CredentialEntry>,
}

#[derive(Debug, Clone)]
pub struct CredentialStore {
    by_access_key: HashMap<String, CredentialEntry>,
}

impl CredentialStore {
    /// Bootstrap from server config and optional on-disk credential file.
    pub async fn load(data_dir: &str, config: &Config) -> anyhow::Result<Self> {
        let mut by_access_key = HashMap::new();

        by_access_key.insert(
            config.access_key.clone(),
            CredentialEntry {
                access_key: config.access_key.clone(),
                secret_key: config.secret_key.clone(),
                enabled: true,
                description: Some("server bootstrap credential".into()),
            },
        );

        let path = format!("{data_dir}/{CREDENTIALS_FILE}");
        if let Ok(raw) = fs::read_to_string(&path).await {
            let file: CredentialsFile = serde_json::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("parse {path}: {e}"))?;
            for entry in file.credentials {
                if entry.access_key.is_empty() || entry.secret_key.is_empty() {
                    continue;
                }
                by_access_key.insert(entry.access_key.clone(), entry);
            }
        }

        Ok(Self { by_access_key })
    }

    pub fn lookup(&self, access_key: &str) -> Option<&CredentialEntry> {
        self.by_access_key
            .get(access_key)
            .filter(|c| c.enabled)
    }

    #[allow(dead_code)]
    pub fn list_access_keys(&self) -> Vec<&str> {
        self.by_access_key
            .values()
            .filter(|c| c.enabled)
            .map(|c| c.access_key.as_str())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.by_access_key.values().filter(|c| c.enabled).count()
    }

    #[cfg(test)]
    pub fn from_single(access_key: &str, secret_key: &str) -> Self {
        let mut by_access_key = HashMap::new();
        by_access_key.insert(
            access_key.to_string(),
            CredentialEntry {
                access_key: access_key.to_string(),
                secret_key: secret_key.to_string(),
                enabled: true,
                description: None,
            },
        );
        Self { by_access_key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config() -> Config {
        Config {
            port: 9000,
            address: "127.0.0.1".into(),
            data_dir: "./data".into(),
            access_key: "primary".into(),
            secret_key: "primary-secret".into(),
            region: "us-east-1".into(),
            master_key: None,
            allow_insecure_dev: true,
            secure_cookies: false,
            erasure_coding: false,
            chunk_size: 10 * 1024 * 1024,
            parity_shards: 0,
            default_buckets: String::new(),
            max_console_body_bytes: 1024 * 1024,
            max_object_bytes: 0,
            min_free_disk_bytes: 0,
            s3_rate_auth_max: 60,
            s3_rate_auth_window_secs: 300,
            s3_rate_put_max: 0,
            s3_rate_put_window_secs: 60,
            admin_token: String::new(),
            admin_rate_max: 120,
            admin_rate_window_secs: 60,
            trusted_proxies: String::new(),
            login_rate_limit_redis_url: None,
            server_host: String::new(),
        }
    }

    #[tokio::test]
    async fn merges_bootstrap_and_file_credentials() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_str().unwrap();
        let path = format!("{dir}/{CREDENTIALS_FILE}");
        fs::write(
            &path,
            r#"{"credentials":[{"access_key":"user2","secret_key":"secret2","enabled":true}]}"#,
        )
        .await
        .unwrap();

        let store = CredentialStore::load(dir, &test_config()).await.unwrap();
        assert!(store.lookup("primary").is_some());
        assert!(store.lookup("user2").is_some());
        assert_eq!(store.len(), 2);
    }

    #[tokio::test]
    async fn disabled_credentials_are_ignored() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_str().unwrap();
        fs::write(
            format!("{dir}/{CREDENTIALS_FILE}"),
            r#"{"credentials":[{"access_key":"gone","secret_key":"x","enabled":false}]}"#,
        )
        .await
        .unwrap();

        let store = CredentialStore::load(dir, &test_config()).await.unwrap();
        assert!(store.lookup("gone").is_none());
    }
}