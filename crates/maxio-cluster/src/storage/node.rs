//! Production storage Raft node (persistent data dir + HTTP transport).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use maxio_storage::filesystem::FilesystemStorage;
use maxio_storage::keys::Keyring;
use maxio_storage::quota::QuotaLimits;
use openraft::Config;
use openraft::Raft;

use crate::storage::api::{RaftApiState, raft_router};
use crate::storage::http_network::HttpRaftNetworkFactory;
use crate::storage::network::ClusterRouter;
use crate::storage::raft_store::StorageRaftStore;
use crate::storage::types::{StorageNodeId, StorageRaft};

/// Live handle shared between Raft task and HTTP API.
#[derive(Clone)]
pub struct StorageRaftNodeHandle {
    pub node_id: StorageNodeId,
    pub raft: StorageRaft,
}

/// Configuration for one production storage Raft peer.
#[derive(Debug, Clone)]
pub struct StorageRaftNodeConfig {
    pub node_id: StorageNodeId,
    pub data_dir: String,
    pub bind_addr: String,
    pub advertise_addr: String,
    pub peer_urls: BTreeMap<StorageNodeId, String>,
    pub voter_ids: BTreeSet<StorageNodeId>,
    pub bootstrap: bool,
    pub erasure_coding: bool,
    pub chunk_size: u64,
    pub parity_shards: u32,
    pub metadata_index: bool,
}

pub fn default_openraft_config() -> anyhow::Result<Arc<Config>> {
    Ok(Arc::new(
        Config {
            heartbeat_interval: 500,
            election_timeout_min: 1500,
            election_timeout_max: 3000,
            ..Default::default()
        }
        .validate()?,
    ))
}

pub struct StorageRaftNode {
    pub handle: StorageRaftNodeHandle,
    pub store: Arc<StorageRaftStore>,
    pub fs: Arc<FilesystemStorage>,
    advertise_addr: String,
}

impl StorageRaftNode {
    pub async fn open(cfg: StorageRaftNodeConfig) -> anyhow::Result<Self> {
        let raft_config = default_openraft_config()?;
        let keyring = Arc::new(Keyring::load(&cfg.data_dir, None).await?);
        let fs = Arc::new(
            FilesystemStorage::new(
                &cfg.data_dir,
                cfg.erasure_coding,
                cfg.chunk_size,
                cfg.parity_shards,
                keyring,
                QuotaLimits::from_config(0, 0),
                cfg.metadata_index,
            )
            .await?,
        );
        let store = Arc::new(StorageRaftStore::new(fs.clone()));
        let (log_store, sm_store) = openraft::storage::Adaptor::new(store.clone());

        let raft = if cfg.peer_urls.is_empty() {
            let router = ClusterRouter::default();
            let raft = Raft::new(
                cfg.node_id,
                raft_config.clone(),
                router.clone(),
                log_store,
                sm_store,
            )
            .await?;
            router
                .register(
                    cfg.node_id,
                    crate::storage::network::RaftHandle { raft: raft.clone() },
                )
                .await;
            raft
        } else {
            let factory = HttpRaftNetworkFactory::new(cfg.peer_urls.clone());
            Raft::new(
                cfg.node_id,
                raft_config.clone(),
                factory,
                log_store,
                sm_store,
            )
            .await?
        };

        if cfg.bootstrap {
            raft.initialize(cfg.voter_ids.clone()).await?;
        }

        let handle = StorageRaftNodeHandle {
            node_id: cfg.node_id,
            raft,
        };

        Ok(Self {
            handle,
            store,
            fs,
            advertise_addr: cfg.advertise_addr,
        })
    }

    pub fn router(&self) -> axum::Router {
        raft_router(RaftApiState {
            handle: self.handle.clone(),
            advertise_addr: self.advertise_addr.clone(),
        })
    }

    pub async fn serve(self, bind_addr: &str) -> anyhow::Result<()> {
        let app = self.router();
        let listener = tokio::net::TcpListener::bind(bind_addr).await?;
        tracing::info!(
            node_id = self.handle.node_id,
            addr = bind_addr,
            advertise = %self.advertise_addr,
            "storage Raft HTTP listener started"
        );
        axum::serve(listener, app).await?;
        Ok(())
    }
}
