//! Distributed erasure coding (P1-18 / P1-19).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use maxio_common::cluster::EcShardPlacement;
use maxio_storage::filesystem::FilesystemStorage;

pub use maxio_common::cluster::EcShardPlacement as EcShardMap;

/// Round-robin placement of K+M shards across `node_ids`.
pub fn place_shards(
    bucket: &str,
    key: &str,
    data_shards: u32,
    parity_shards: u32,
    node_ids: &[String],
) -> EcShardPlacement {
    let total = data_shards + parity_shards;
    let mut placements = Vec::with_capacity(total as usize);
    for shard in 0..total {
        let node = &node_ids[shard as usize % node_ids.len()];
        placements.push((shard, node.clone()));
    }
    EcShardPlacement {
        bucket: bucket.into(),
        key: key.into(),
        data_shards,
        parity_shards,
        placements,
    }
}

/// Write raw shard bytes to the owning node's data directory.
pub async fn write_shard(
    nodes: &HashMap<String, Arc<FilesystemStorage>>,
    placement: &EcShardPlacement,
    shard_index: u32,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let owner = placement
        .placements
        .iter()
        .find(|(i, _)| *i == shard_index)
        .map(|(_, n)| n.clone())
        .ok_or_else(|| anyhow::anyhow!("shard {shard_index} not placed"))?;
    let fs = nodes
        .get(&owner)
        .ok_or_else(|| anyhow::anyhow!("unknown node {owner}"))?;
    let path = shard_path(
        fs.data_root(),
        &placement.bucket,
        &placement.key,
        shard_index,
    );
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    tokio::fs::write(&path, bytes).await?;
    Ok(())
}

/// Read shard from local disk or peer node data root (P1-19).
pub async fn read_shard(
    nodes: &HashMap<String, Arc<FilesystemStorage>>,
    placement: &EcShardPlacement,
    shard_index: u32,
) -> anyhow::Result<Vec<u8>> {
    let owner = placement
        .placements
        .iter()
        .find(|(i, _)| *i == shard_index)
        .map(|(_, n)| n.clone())
        .ok_or_else(|| anyhow::anyhow!("shard {shard_index} not placed"))?;
    let fs = nodes
        .get(&owner)
        .ok_or_else(|| anyhow::anyhow!("unknown node {owner}"))?;
    let path = shard_path(
        fs.data_root(),
        &placement.bucket,
        &placement.key,
        shard_index,
    );
    match tokio::fs::read(&path).await {
        Ok(b) => Ok(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Peer fetch: try any other node (simulated remote RPC).
            for (node_id, peer) in nodes {
                if node_id == &owner {
                    continue;
                }
                let alt = shard_path(
                    peer.data_root(),
                    &placement.bucket,
                    &placement.key,
                    shard_index,
                );
                if let Ok(b) = tokio::fs::read(&alt).await {
                    return Ok(b);
                }
            }
            Err(e.into())
        }
        Err(e) => Err(e.into()),
    }
}

fn shard_path(data_root: &Path, bucket: &str, key: &str, shard: u32) -> std::path::PathBuf {
    data_root
        .join(".cluster-shards")
        .join(bucket)
        .join(key)
        .join(format!("{shard:06}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_spreads_across_nodes() {
        let map = place_shards("b", "k", 2, 1, &["n1".into(), "n2".into(), "n3".into()]);
        let owners: Vec<_> = map.placements.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(owners.len(), 3);
        assert_ne!(owners[0], owners[1]);
    }
}
