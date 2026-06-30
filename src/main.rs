use clap::Parser;
use clap::Subcommand;
use http::Uri;
use maxio::{auth, config::Config, rate_limit, server, storage};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "maxio",
    about = "S3-compatible object storage server",
    version = maxio::version::VERSION
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    config: Config,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the HTTP/S3 server (default when no subcommand is provided)
    Serve,

    /// Run a storage-tier Raft peer (metadata quorum, production cluster)
    StorageRaft {
        #[command(flatten)]
        config: StorageRaftCli,
    },

    /// Check server health by sending an HTTP GET request
    Healthcheck {
        /// Healthcheck endpoint URL
        #[arg(long, env = "MAXIO_HEALTHCHECK_URL", default_value_t = default_healthcheck_url())]
        url: String,

        /// Timeout in milliseconds for connect/read operations
        #[arg(long, env = "MAXIO_HEALTHCHECK_TIMEOUT_MS", default_value = "2000")]
        timeout_ms: u64,
    },

    /// Manage the SSE-S3 master-key keyring
    #[command(subcommand)]
    Keyring(KeyringCmd),
}

#[derive(Subcommand, Debug)]
enum KeyringCmd {
    /// Generate a new master key, mark it active, and demote the previous
    /// active key (retained so existing objects keep decrypting).
    /// Restart the server after rotating to pick up the new active key.
    Rotate {
        /// Data directory containing .maxio-keys.json
        #[arg(long, env = "MAXIO_DATA_DIR", default_value = "./data")]
        data_dir: String,
    },

    /// Print the keyring file contents (key ids + metadata, never the key
    /// material itself).
    List {
        #[arg(long, env = "MAXIO_DATA_DIR", default_value = "./data")]
        data_dir: String,
    },
}

#[derive(clap::Parser, Debug, Clone)]
struct StorageRaftCli {
    /// Root data directory for this storage node
    #[arg(long, env = "MAXIO_DATA_DIR", default_value = "./data")]
    data_dir: String,

    /// Numeric Raft node id (unique per storage peer)
    #[arg(long, env = "MAXIO_STORAGE_RAFT_NODE_ID")]
    node_id: u64,

    /// HTTP bind address for Raft RPC + status (`host:port`)
    #[arg(long, env = "MAXIO_STORAGE_RAFT_BIND", default_value = "0.0.0.0:9100")]
    bind: String,

    /// Address advertised to server tier (`host:port`). Defaults to bind address.
    #[arg(long, env = "MAXIO_STORAGE_RAFT_ADVERTISE")]
    advertise: Option<String>,

    /// Peer base URLs: `1=http://storage-1:9100,2=http://storage-2:9100`
    #[arg(long, env = "MAXIO_STORAGE_RAFT_PEERS", default_value = "")]
    peers: String,

    /// Voter ids for bootstrap (comma-separated). Required when bootstrap is true.
    #[arg(long, env = "MAXIO_STORAGE_RAFT_VOTERS", default_value = "1,2,3")]
    voters: String,

    /// Initialize the Raft cluster (run once on the first storage node only)
    #[arg(long, env = "MAXIO_STORAGE_RAFT_BOOTSTRAP", default_value = "false")]
    bootstrap: bool,

    #[arg(long, env = "MAXIO_ERASURE_CODING", default_value = "false")]
    erasure_coding: bool,

    #[arg(long, env = "MAXIO_CHUNK_SIZE", default_value = "10485760")]
    chunk_size: u64,

    #[arg(long, env = "MAXIO_PARITY_SHARDS", default_value = "0")]
    parity_shards: u32,

    #[arg(long, env = "MAXIO_METADATA_INDEX", default_value = "false")]
    metadata_index: bool,

    /// Enable background EC bitrot scanner on this storage node (P1-25)
    #[arg(long, env = "MAXIO_BITROT_SCAN_ENABLED", default_value = "false")]
    bitrot_scan_enabled: bool,

    /// Interval between EC bitrot scan passes (seconds)
    #[arg(long, env = "MAXIO_BITROT_SCAN_INTERVAL_SECS", default_value = "3600")]
    bitrot_scan_interval_secs: u64,
}

fn default_healthcheck_url() -> String {
    let port = std::env::var("MAXIO_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(9000);
    format!("http://127.0.0.1:{}/healthz", port)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Serve) | None => {}
        Some(Commands::Healthcheck { url, timeout_ms }) => {
            return run_healthcheck(&url, timeout_ms).await;
        }
        Some(Commands::Keyring(KeyringCmd::Rotate { ref data_dir })) => {
            return run_keyring_rotate(data_dir).await;
        }
        Some(Commands::Keyring(KeyringCmd::List { ref data_dir })) => {
            return run_keyring_list(data_dir).await;
        }
        Some(Commands::StorageRaft { config }) => {
            return run_storage_raft(config).await;
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = cli.config;
    config.validate_keycloak()?;

    if config.access_key == "maxioadmin"
        && config.secret_key == "maxioadmin"
        && !config.allow_insecure_dev
    {
        anyhow::bail!(
            "refusing to start with default credentials in production; set MAXIO_ACCESS_KEY/MAXIO_SECRET_KEY or use --allow-insecure-dev for local development"
        );
    }

    tokio::fs::create_dir_all(&config.data_dir).await?;

    // Build the SSE-S3 keyring (bootstrap a random master key on first run).
    let keyring = Arc::new(
        storage::keys::Keyring::load(&config.data_dir, config.master_key.as_deref()).await?,
    );
    if config.master_key.is_none() {
        tracing::info!(
            "SSE-S3 keyring: active key id {} (file {}/.maxio-keys.json — BACK THIS UP)",
            keyring.active_id(),
            config.data_dir
        );
    } else {
        tracing::info!(
            "SSE-S3 keyring: active key id {} (from MAXIO_MASTER_KEY)",
            keyring.active_id()
        );
    }

    let quota = storage::quota::QuotaLimits::from_config(
        config.max_object_bytes,
        config.min_free_disk_bytes,
    );
    let kms = storage::kms::load_from_env()
        .map_err(|e| anyhow::anyhow!("failed to load KMS from MAXIO_KMS_MASTER_KEY: {e}"))?;
    if kms.is_some() {
        tracing::info!("SSE-KMS enabled via MAXIO_KMS_MASTER_KEY");
    }

    let fs_storage = storage::filesystem::FilesystemStorage::new(
        &config.data_dir,
        config.erasure_coding,
        config.chunk_size,
        config.parity_shards,
        keyring.clone(),
        kms,
        quota,
        config.metadata_index,
    )
    .await?;

    let mut storage = storage::backend::dyn_storage(fs_storage);
    if config.cluster_mode && !config.storage_endpoints.is_empty() {
        let peers = maxio_cluster::routing::parse_storage_peers(&config.storage_endpoints)?;
        let raft = maxio_cluster::StorageRaftClient::new(peers);
        storage = maxio_cluster::wrap_cluster_storage(storage, raft);
        tracing::info!("cluster mode: bucket metadata mutations route to storage Raft leader");
    }
    storage::provision_default_buckets(storage.as_ref(), &config.default_buckets, &config.region)
        .await;

    let login_rate_limiter = Arc::new(
        rate_limit::LoginRateLimiter::from_config(config.login_rate_limit_redis_url.as_deref())
            .await?,
    );
    let credentials =
        Arc::new(auth::credentials::CredentialStore::load(&config.data_dir, &config).await?);
    if !credentials.is_empty() && credentials.len() > 1 {
        tracing::info!(
            "S3 credentials: {} access key(s) loaded (bootstrap + .maxio-credentials.json)",
            credentials.len()
        );
        tracing::debug!(
            access_keys = ?credentials.list_access_keys(),
            "loaded S3 access keys"
        );
    }

    let keycloak = if config.keycloak_enabled {
        let auth = auth::keycloak::KeycloakAuth::from_config(&config)?;
        tracing::info!(
            realm = %config.keycloak_realm,
            client_id = %config.keycloak_client_id,
            issuer = %auth.settings().issuer_url(),
            "Keycloak OIDC enabled for console authentication"
        );
        Some(Arc::new(auth))
    } else {
        None
    };

    let addr = format!("{}:{}", config.address, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let listen_port = listener.local_addr()?.port();
    let state = server::new_app_state(
        storage,
        Arc::new(config.clone()),
        login_rate_limiter,
        credentials,
        keycloak,
        Some(listen_port),
    );

    if config.cluster_mode {
        let sync_state = state.clone();
        tokio::spawn(async move {
            maxio::cluster_sync::run_cluster_sync(sync_state).await;
        });
    }

    // Background housekeeping: abort stale multipart uploads (>7 days) and
    // remove leftover temp files from crashed writes. Runs once at startup,
    // then hourly.
    {
        let storage = state.storage.clone();
        let last_run = state.last_housekeeping_at.clone();
        tokio::spawn(async move {
            let stale_after = chrono::Duration::days(7);
            let mut ticker =
                tokio::time::interval(Duration::from_secs(server::HOUSEKEEPING_INTERVAL_SECS));
            loop {
                ticker.tick().await;
                let (uploads, temps, expired) = storage.housekeeping_sweep(stale_after).await;
                last_run.store(
                    chrono::Utc::now().timestamp(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                if uploads > 0 || temps > 0 || expired > 0 {
                    tracing::info!(
                        "housekeeping: removed {} stale upload(s), {} temp file(s), {} expired object(s)",
                        uploads,
                        temps,
                        expired
                    );
                }
            }
        });
    }

    if config.metrics_port > 0 {
        let metrics_state = state.clone();
        let metrics_addr = format!("{}:{}", config.address, config.metrics_port);
        tokio::spawn(async move {
            match tokio::net::TcpListener::bind(&metrics_addr).await {
                Ok(listener) => {
                    tracing::info!("Metrics listener on {}", metrics_addr);
                    let app = server::metrics_router(metrics_state);
                    if let Err(err) = axum::serve(listener, app.into_make_service()).await {
                        tracing::error!("metrics server error: {err}");
                    }
                }
                Err(err) => tracing::error!("failed to bind metrics port {}: {err}", metrics_addr),
            }
        });
    }

    let app = server::build_router(state);
    if config.access_key == "maxioadmin" && config.secret_key == "maxioadmin" {
        tracing::warn!(
            "WARNING: Using default credentials because insecure development mode is enabled."
        );
    }

    tracing::info!("MaxIO v{} listening on {}", maxio::version::VERSION, addr);
    tracing::info!("Access Key: {}", config.access_key);
    tracing::info!("Secret Key: [REDACTED]");
    tracing::info!("Data dir:   {}", config.data_dir);
    tracing::info!("Region:     {}", config.region);
    if config.erasure_coding {
        tracing::info!(
            "Erasure coding: enabled (chunk size: {}MB)",
            config.chunk_size / (1024 * 1024)
        );
        if config.parity_shards > 0 {
            tracing::info!(
                "Parity shards: {} (can tolerate {} lost/corrupt chunks per object)",
                config.parity_shards,
                config.parity_shards
            );
        }
    } else if config.parity_shards > 0 {
        tracing::warn!("--parity-shards ignored: requires --erasure-coding to be enabled");
    }
    let display_host = if config.address == "0.0.0.0" {
        "localhost"
    } else {
        &config.address
    };
    tracing::info!("Web UI:     http://{}:{}/ui/", display_host, config.port);
    if config.metrics_enabled {
        tracing::info!(
            "Metrics:    http://{}:{}/metrics",
            display_host,
            config.port
        );
    }
    if config.audit_log {
        tracing::info!("Audit log:  enabled (target=maxio_audit, JSON lines)");
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn run_healthcheck(url: &str, timeout_ms: u64) -> anyhow::Result<()> {
    let uri: Uri = url.parse()?;
    if uri.scheme_str() != Some("http") {
        anyhow::bail!("unsupported scheme in healthcheck URL: only http is supported");
    }

    let host = uri
        .host()
        .ok_or_else(|| anyhow::anyhow!("healthcheck URL is missing host"))?;
    let port = uri.port_u16().unwrap_or(80);
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let timeout_duration = Duration::from_millis(timeout_ms);

    let mut stream: TcpStream = timeout(timeout_duration, TcpStream::connect((host, port)))
        .await
        .map_err(|_| anyhow::anyhow!("healthcheck connect timeout after {}ms", timeout_ms))??;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: maxio-healthcheck/{}\r\n\r\n",
        path_and_query,
        host,
        maxio::version::VERSION
    );
    timeout(timeout_duration, stream.write_all(request.as_bytes()))
        .await
        .map_err(|_| anyhow::anyhow!("healthcheck write timeout after {}ms", timeout_ms))??;

    let mut response = Vec::new();
    timeout(timeout_duration, stream.read_to_end(&mut response))
        .await
        .map_err(|_| anyhow::anyhow!("healthcheck read timeout after {}ms", timeout_ms))??;

    let status_line = String::from_utf8_lossy(&response)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid HTTP response from {}", url))?;

    if (200..300).contains(&status_code) {
        println!("ok");
        return Ok(());
    }

    anyhow::bail!("healthcheck failed with HTTP status {}", status_code);
}

async fn run_keyring_rotate(data_dir: &str) -> anyhow::Result<()> {
    let result = storage::keys::rotate(data_dir).await?;
    println!("✓ keyring rotated at {}/.maxio-keys.json", data_dir);
    println!("  new active key id: {}", result.new_active_id);
    match result.previous_active_id {
        Some(prev) => println!(
            "  previous active:   {} (retained for old-object decryption)",
            prev
        ),
        None => println!("  previous active:   <none> (first key in ring)"),
    }
    println!("  total keys in ring: {}", result.total_keys);
    println!();
    println!("Restart the server to begin encrypting new objects with the new key.");
    Ok(())
}

async fn run_storage_raft(cfg: StorageRaftCli) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tokio::fs::create_dir_all(&cfg.data_dir).await?;

    let peer_urls = maxio_cluster::parse_raft_peer_urls(&cfg.peers)?;
    let voter_ids: std::collections::BTreeSet<u64> = cfg
        .voters
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if cfg.bootstrap && voter_ids.is_empty() {
        anyhow::bail!("MAXIO_STORAGE_RAFT_BOOTSTRAP requires non-empty MAXIO_STORAGE_RAFT_VOTERS");
    }

    let advertise = cfg.advertise.clone().unwrap_or_else(|| cfg.bind.clone());

    let node = maxio_cluster::StorageRaftNode::open(maxio_cluster::StorageRaftNodeConfig {
        node_id: cfg.node_id,
        data_dir: cfg.data_dir.clone(),
        bind_addr: cfg.bind.clone(),
        advertise_addr: advertise,
        peer_urls,
        voter_ids,
        bootstrap: cfg.bootstrap,
        erasure_coding: cfg.erasure_coding,
        chunk_size: cfg.chunk_size,
        parity_shards: cfg.parity_shards,
        metadata_index: cfg.metadata_index,
        bitrot_scan_enabled: cfg.bitrot_scan_enabled,
        bitrot_scan_interval_secs: cfg.bitrot_scan_interval_secs,
    })
    .await?;

    let bitrot = maxio_cluster::BitrotScannerConfig {
        local_node_id: cfg.node_id.to_string(),
        interval: std::time::Duration::from_secs(cfg.bitrot_scan_interval_secs),
        enabled: cfg.bitrot_scan_enabled && cfg.erasure_coding && cfg.parity_shards > 0,
    };
    node.serve(&cfg.bind, bitrot).await
}

async fn run_keyring_list(data_dir: &str) -> anyhow::Result<()> {
    let path = format!("{}/.maxio-keys.json", data_dir);
    let data = match tokio::fs::read_to_string(&path).await {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("No keyring file yet at {}", path);
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    // Minimal pretty-print: parse, strip key_b64 fields, show id/created/active.
    let v: serde_json::Value = serde_json::from_str(&data)?;
    let empty = Vec::new();
    let entries = v.get("keys").and_then(|k| k.as_array()).unwrap_or(&empty);
    println!("{:<20}  {:<26}  ACTIVE", "KEY_ID", "CREATED_AT");
    for e in entries {
        let id = e.get("id").and_then(|x| x.as_str()).unwrap_or("?");
        let created = e.get("created_at").and_then(|x| x.as_str()).unwrap_or("?");
        let active = e.get("active").and_then(|x| x.as_bool()).unwrap_or(false);
        println!(
            "{:<20}  {:<26}  {}",
            id,
            created,
            if active { "yes" } else { "no" }
        );
    }
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    tracing::info!("Shutdown signal received, draining connections...");
}
