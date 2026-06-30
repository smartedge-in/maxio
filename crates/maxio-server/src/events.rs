//! S3 event notifications (P3-27): durable spool + webhook delivery.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::storage::BucketNotificationConfig;

const SPOOL_DIR_NAME: &str = ".maxio-event-spool";
const DRAIN_INTERVAL_SECS: u64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: String,
    pub event: String,
    pub bucket: String,
    pub key: String,
    pub time: String,
    pub size: Option<u64>,
    pub etag: Option<String>,
    pub webhook_url: String,
}

#[derive(Debug, Clone)]
pub struct EventSpool {
    dir: PathBuf,
    client: reqwest::Client,
}

impl EventSpool {
    pub fn open(data_dir: &str) -> Self {
        Self {
            dir: Path::new(data_dir).join(SPOOL_DIR_NAME),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub async fn ensure_dir(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir).await
    }

    pub async fn enqueue(&self, record: &EventRecord) -> std::io::Result<()> {
        self.ensure_dir().await?;
        let path = self.dir.join(format!("{}.json", record.id));
        let json = serde_json::to_string_pretty(record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(&path, json).await
    }

    pub async fn drain_once(&self) -> u64 {
        let mut delivered = 0u64;
        let mut entries = match fs::read_dir(&self.dir).await {
            Ok(e) => e,
            Err(_) => return 0,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let data = match fs::read_to_string(&path).await {
                Ok(d) => d,
                Err(_) => continue,
            };
            let record: EventRecord = match serde_json::from_str(&data) {
                Ok(r) => r,
                Err(_) => {
                    let _ = fs::remove_file(&path).await;
                    continue;
                }
            };
            match self
                .client
                .post(&record.webhook_url)
                .json(&record)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let _ = fs::remove_file(&path).await;
                    delivered += 1;
                }
                Ok(resp) => {
                    tracing::warn!(
                        webhook = %record.webhook_url,
                        status = %resp.status(),
                        event_id = %record.id,
                        "event webhook delivery failed"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        webhook = %record.webhook_url,
                        error = %e,
                        event_id = %record.id,
                        "event webhook request error"
                    );
                }
            }
        }
        delivered
    }
}

pub fn spawn_drain_task(spool: Arc<EventSpool>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(DRAIN_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            let n = spool.drain_once().await;
            if n > 0 {
                tracing::debug!("event spool: delivered {n} notification(s)");
            }
        }
    });
}

/// Returns true when `url` resolves to RFC1918, loopback, or link-local (internal).
pub fn is_internal_webhook_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    if host == "localhost" {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_internal_ip(ip);
    }
    false
}

fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() || v4.is_private() || v4.is_link_local() {
                return true;
            }
            let o = v4.octets();
            o[0] == 169 && o[1] == 254
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local(),
    }
}

pub fn validate_webhook_url(url: &str, allow_external: bool) -> Result<(), String> {
    if url.is_empty() {
        return Err("webhook URL must not be empty".into());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("webhook URL must use http or https".into());
    }
    if allow_external || is_internal_webhook_url(url) {
        Ok(())
    } else {
        Err(
            "webhook URL must target an internal address (RFC1918/localhost); \
             set MAXIO_ALLOW_EXTERNAL_WEBHOOKS=1 to override"
                .into(),
        )
    }
}

fn event_matches(config: &BucketNotificationConfig, event_name: &str) -> bool {
    config.events.iter().any(|e| {
        if e == event_name {
            return true;
        }
        if e.ends_with(":*") {
            let prefix = e.trim_end_matches(":*");
            return event_name.starts_with(prefix);
        }
        false
    })
}

pub async fn emit_object_created(state: &AppState, bucket: &str, key: &str, size: u64, etag: &str) {
    let Ok(Some(config)) = state.storage.get_bucket_notification(bucket).await else {
        return;
    };
    if !event_matches(&config, "s3:ObjectCreated:Put") {
        return;
    }
    let record = EventRecord {
        id: Uuid::new_v4().to_string(),
        event: "s3:ObjectCreated:Put".to_string(),
        bucket: bucket.to_string(),
        key: key.to_string(),
        time: chrono::Utc::now().to_rfc3339(),
        size: Some(size),
        etag: Some(etag.to_string()),
        webhook_url: config.webhook_url.clone(),
    };
    if let Err(e) = state.event_spool.enqueue(&record).await {
        tracing::warn!(bucket, key, error = %e, "failed to enqueue ObjectCreated event");
    }
}

pub async fn emit_object_removed(state: &AppState, bucket: &str, key: &str) {
    let Ok(Some(config)) = state.storage.get_bucket_notification(bucket).await else {
        return;
    };
    if !event_matches(&config, "s3:ObjectRemoved:Delete") {
        return;
    }
    let record = EventRecord {
        id: Uuid::new_v4().to_string(),
        event: "s3:ObjectRemoved:Delete".to_string(),
        bucket: bucket.to_string(),
        key: key.to_string(),
        time: chrono::Utc::now().to_rfc3339(),
        size: None,
        etag: None,
        webhook_url: config.webhook_url.clone(),
    };
    if let Err(e) = state.event_spool.enqueue(&record).await {
        tracing::warn!(bucket, key, error = %e, "failed to enqueue ObjectRemoved event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_rfc1918_webhook() {
        assert!(is_internal_webhook_url("http://192.168.1.10/hook"));
        assert!(is_internal_webhook_url("http://10.0.0.5/events"));
        assert!(is_internal_webhook_url("http://127.0.0.1:9999/"));
        assert!(is_internal_webhook_url("http://localhost/hook"));
    }

    #[test]
    fn rejects_public_webhook_without_override() {
        assert!(!is_internal_webhook_url("https://example.com/hook"));
        assert!(validate_webhook_url("https://example.com/hook", false).is_err());
        assert!(validate_webhook_url("https://example.com/hook", true).is_ok());
    }
}
