//! Cluster-aware EC bitrot scanner and heal (P1-25).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{reconstruct_shard, shard_path};
use maxio_common::cluster::EcShardPlacement;
use maxio_storage::filesystem::FilesystemStorage;
use sha2::{Digest, Sha256};

const CHECKSUM_SUFFIX: &str = ".sha256";

/// Prometheus-friendly counters for bitrot scanning.
#[derive(Debug, Default)]
pub struct BitrotMetrics {
    pub shards_scanned: AtomicU64,
    pub corrupt_detected: AtomicU64,
    pub shards_healed: AtomicU64,
    pub scan_errors: AtomicU64,
}

impl BitrotMetrics {
    pub fn render_prometheus(&self) -> String {
        format!(
            "# HELP maxio_ec_bitrot_shards_scanned_total EC shard files verified by bitrot scanner\n\
             # TYPE maxio_ec_bitrot_shards_scanned_total counter\n\
             maxio_ec_bitrot_shards_scanned_total {}\n\
             # HELP maxio_ec_bitrot_corrupt_detected_total EC shards with checksum mismatch\n\
             # TYPE maxio_ec_bitrot_corrupt_detected_total counter\n\
             maxio_ec_bitrot_corrupt_detected_total {}\n\
             # HELP maxio_ec_bitrot_shards_healed_total EC shards rebuilt from parity/peers\n\
             # TYPE maxio_ec_bitrot_shards_healed_total counter\n\
             maxio_ec_bitrot_shards_healed_total {}\n\
             # HELP maxio_ec_bitrot_scan_errors_total Bitrot scanner errors (I/O, insufficient parity)\n\
             # TYPE maxio_ec_bitrot_scan_errors_total counter\n\
             maxio_ec_bitrot_scan_errors_total {}\n",
            self.shards_scanned.load(Ordering::Relaxed),
            self.corrupt_detected.load(Ordering::Relaxed),
            self.shards_healed.load(Ordering::Relaxed),
            self.scan_errors.load(Ordering::Relaxed),
        )
    }
}

#[derive(Debug, Clone)]
pub struct BitrotScannerConfig {
    pub local_node_id: String,
    pub interval: Duration,
    pub enabled: bool,
}

pub fn checksum_sidecar(shard_file: &Path) -> PathBuf {
    let name = shard_file
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("shard");
    shard_file.with_file_name(format!("{name}{CHECKSUM_SUFFIX}"))
}

pub fn shard_checksum_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

pub async fn write_checksum_sidecar(shard_file: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let sidecar = checksum_sidecar(shard_file);
    if let Some(parent) = sidecar.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let digest = shard_checksum_hex(bytes);
    tokio::fs::write(&sidecar, format!("{digest}\n")).await?;
    Ok(())
}

/// Returns `Ok(true)` when checksum matches, `Ok(false)` when corrupt or missing sidecar mismatch.
pub async fn verify_shard_checksum(shard_file: &Path, bytes: &[u8]) -> anyhow::Result<bool> {
    let sidecar = checksum_sidecar(shard_file);
    let expected = match tokio::fs::read_to_string(&sidecar).await {
        Ok(s) => s.trim().to_ascii_lowercase(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            write_checksum_sidecar(shard_file, bytes).await?;
            return Ok(true);
        }
        Err(e) => return Err(e.into()),
    };
    Ok(shard_checksum_hex(bytes) == expected)
}

/// Fetch shard bytes from a peer storage node over HTTP (`GET /internal/shard/...`).
pub async fn fetch_shard_http(
    client: &reqwest::Client,
    peer_base_url: &str,
    bucket: &str,
    key: &str,
    shard_index: u32,
) -> anyhow::Result<Vec<u8>> {
    let base = peer_base_url.trim_end_matches('/');
    let url = format!("{base}/internal/shard?bucket={bucket}&key={key}&index={shard_index}");
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}

/// Gather shards for reconstruction from local disk and optional HTTP peers.
pub async fn gather_shards(
    local_fs: &FilesystemStorage,
    placement: &EcShardPlacement,
    local_node_id: &str,
    peer_urls: &BTreeMap<u64, String>,
    nodes: Option<&HashMap<String, Arc<FilesystemStorage>>>,
    skip_index: Option<u32>,
) -> anyhow::Result<Vec<(u32, Vec<u8>)>> {
    let client = reqwest::Client::new();
    let mut present = Vec::new();

    for (idx, owner) in &placement.placements {
        if skip_index == Some(*idx) {
            continue;
        }
        let path = shard_path(
            local_fs.data_root(),
            &placement.bucket,
            &placement.key,
            *idx,
        );
        if owner == local_node_id
            && let Ok(b) = tokio::fs::read(&path).await
        {
            present.push((*idx, b));
            continue;
        }
        if let Some(map) = nodes
            && let Ok(b) = super::read_shard(map, placement, *idx).await
        {
            present.push((*idx, b));
            continue;
        }
        if let Ok(node_id) = owner.parse::<u64>()
            && let Some(base) = peer_urls.get(&node_id)
            && let Ok(b) =
                fetch_shard_http(&client, base, &placement.bucket, &placement.key, *idx).await
        {
            present.push((*idx, b));
        }
    }
    Ok(present)
}

fn shard_size_from_present(present: &[(u32, Vec<u8>)]) -> usize {
    present.iter().map(|(_, b)| b.len()).max().unwrap_or(0)
}

/// Rebuild and rewrite a corrupt local shard using parity/peers.
pub async fn heal_local_shard(
    local_fs: &FilesystemStorage,
    placement: &EcShardPlacement,
    local_node_id: &str,
    corrupt_index: u32,
    peer_urls: &BTreeMap<u64, String>,
    nodes: Option<&HashMap<String, Arc<FilesystemStorage>>>,
) -> anyhow::Result<()> {
    let present = gather_shards(
        local_fs,
        placement,
        local_node_id,
        peer_urls,
        nodes,
        Some(corrupt_index),
    )
    .await?;
    let shard_size = shard_size_from_present(&present);
    if shard_size == 0 {
        anyhow::bail!("no peer shards available to heal shard {corrupt_index}");
    }
    let rebuilt = reconstruct_shard(
        placement.data_shards,
        placement.parity_shards,
        shard_size,
        &present,
        corrupt_index,
    )?;
    let path = shard_path(
        local_fs.data_root(),
        &placement.bucket,
        &placement.key,
        corrupt_index,
    );
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    tokio::fs::write(&path, &rebuilt).await?;
    write_checksum_sidecar(&path, &rebuilt).await?;
    Ok(())
}

/// Scan placements and heal corrupt shards owned by `local_node_id`.
pub async fn scan_placements(
    local_fs: &FilesystemStorage,
    local_node_id: &str,
    placements: &[EcShardPlacement],
    peer_urls: &BTreeMap<u64, String>,
    nodes: Option<&HashMap<String, Arc<FilesystemStorage>>>,
    metrics: &BitrotMetrics,
) {
    for placement in placements {
        for (idx, owner) in &placement.placements {
            if owner != local_node_id {
                continue;
            }
            let path = shard_path(
                local_fs.data_root(),
                &placement.bucket,
                &placement.key,
                *idx,
            );
            let bytes = match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(
                        bucket = %placement.bucket,
                        key = %placement.key,
                        shard = idx,
                        "bitrot scan: cannot read shard: {e}"
                    );
                    metrics.scan_errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            metrics.shards_scanned.fetch_add(1, Ordering::Relaxed);
            match verify_shard_checksum(&path, &bytes).await {
                Ok(true) => {}
                Ok(false) => {
                    metrics.corrupt_detected.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        bucket = %placement.bucket,
                        key = %placement.key,
                        shard = idx,
                        "bitrot: checksum mismatch — healing"
                    );
                    match heal_local_shard(
                        local_fs,
                        placement,
                        local_node_id,
                        *idx,
                        peer_urls,
                        nodes,
                    )
                    .await
                    {
                        Ok(()) => {
                            metrics.shards_healed.fetch_add(1, Ordering::Relaxed);
                            tracing::info!(
                                bucket = %placement.bucket,
                                key = %placement.key,
                                shard = idx,
                                "bitrot: shard healed"
                            );
                        }
                        Err(e) => {
                            metrics.scan_errors.fetch_add(1, Ordering::Relaxed);
                            tracing::error!(
                                bucket = %placement.bucket,
                                key = %placement.key,
                                shard = idx,
                                "bitrot heal failed: {e}"
                            );
                        }
                    }
                }
                Err(e) => {
                    metrics.scan_errors.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        bucket = %placement.bucket,
                        key = %placement.key,
                        shard = idx,
                        "bitrot verify error: {e}"
                    );
                }
            }
        }
    }
}

/// Spawn a background bitrot scanner on a storage Raft node.
pub fn spawn_bitrot_scanner(
    cfg: BitrotScannerConfig,
    store: Arc<crate::storage::raft_store::StorageRaftStore>,
    fs: Arc<FilesystemStorage>,
    peer_urls: BTreeMap<u64, String>,
    metrics: Arc<BitrotMetrics>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if !cfg.enabled {
            tracing::info!("EC bitrot scanner disabled");
            return;
        }
        tracing::info!(
            node_id = %cfg.local_node_id,
            interval_secs = cfg.interval.as_secs(),
            "EC bitrot scanner started"
        );
        let mut ticker = tokio::time::interval(cfg.interval);
        loop {
            ticker.tick().await;
            let placements = store.all_shard_maps().await;
            if placements.is_empty() {
                continue;
            }
            scan_placements(
                &fs,
                &cfg.local_node_id,
                &placements,
                &peer_urls,
                None,
                &metrics,
            )
            .await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_hex_stable() {
        assert_eq!(
            shard_checksum_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2b264e9b8c1b8e78d82c7b6916e8491adac3a856f"
        );
    }

    #[tokio::test]
    async fn verify_detects_corruption() {
        let tmp = tempfile::tempdir().unwrap();
        let shard = tmp.path().join("000000");
        tokio::fs::write(&shard, b"good").await.unwrap();
        write_checksum_sidecar(&shard, b"good").await.unwrap();
        assert!(verify_shard_checksum(&shard, b"good").await.unwrap());
        assert!(!verify_shard_checksum(&shard, b"bad!").await.unwrap());
    }
}
