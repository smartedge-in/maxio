use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Instant;

use crate::auth::credentials::CredentialStore;
use crate::auth::keycloak::KeycloakAuth;
use crate::cluster::ClusterState;
use crate::config::Config;
use crate::events::{EventSpool, spawn_drain_task};
use crate::metrics::Metrics;
use crate::proxy::TrustedProxies;
use crate::rate_limit::{AdminRateLimiter, LoginRateLimiter, S3RateLimiter};
use crate::storage::backend::DynStorage;

#[derive(Clone)]
pub struct AppState {
    pub storage: DynStorage,
    pub config: Arc<Config>,
    pub login_rate_limiter: Arc<LoginRateLimiter>,
    pub s3_rate_limiter: Arc<S3RateLimiter>,
    pub admin_rate_limiter: Arc<AdminRateLimiter>,
    pub trusted_proxies: Arc<TrustedProxies>,
    pub started_at: Instant,
    pub last_housekeeping_at: Arc<AtomicI64>,
    pub credentials: Arc<CredentialStore>,
    pub keycloak: Option<Arc<KeycloakAuth>>,
    pub server_host: String,
    pub metrics: Arc<Metrics>,
    pub cluster: Option<ClusterState>,
    pub event_spool: Arc<EventSpool>,
}

pub fn new_app_state(
    storage: DynStorage,
    config: Arc<Config>,
    login_rate_limiter: Arc<LoginRateLimiter>,
    credentials: Arc<CredentialStore>,
    keycloak: Option<Arc<KeycloakAuth>>,
    listen_port: Option<u16>,
) -> AppState {
    let event_spool = Arc::new(EventSpool::open(&config.data_dir));
    spawn_drain_task(event_spool.clone());

    AppState {
        storage,
        config: config.clone(),
        login_rate_limiter,
        s3_rate_limiter: Arc::new(S3RateLimiter::from_config(
            config.s3_rate_auth_max,
            config.s3_rate_auth_window_secs,
            config.s3_rate_put_max,
            config.s3_rate_put_window_secs,
        )),
        admin_rate_limiter: Arc::new(AdminRateLimiter::from_config(
            config.admin_rate_max,
            config.admin_rate_window_secs,
        )),
        trusted_proxies: Arc::new(TrustedProxies::parse(&config.trusted_proxies)),
        started_at: Instant::now(),
        last_housekeeping_at: Arc::new(AtomicI64::new(0)),
        credentials,
        keycloak,
        server_host: crate::api::virtual_host::effective_server_host(&config, listen_port),
        metrics: Arc::new(Metrics::default()),
        cluster: config.cluster_mode.then(ClusterState::new),
        event_spool,
    }
}
