//! HTTP-level cluster `/readyz` checks (P1-20).

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use maxio_common::cluster::{RoutingSnapshot, StorageEndpoint};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use crate::auth::credentials::CredentialStore;
    use crate::config::Config;
    use crate::server::{build_router, new_app_state};
    use crate::storage::backend::dyn_storage;
    use crate::storage::filesystem::FilesystemStorage;
    use crate::storage::keys::Keyring;
    use crate::storage::quota::QuotaLimits;

    async fn cluster_app_state(tmp: &TempDir, quorum_ok: bool) -> crate::app_state::AppState {
        let data_dir = tmp.path().to_str().unwrap().to_string();
        let keyring = Arc::new(Keyring::load(&data_dir, None).await.unwrap());
        let fs = FilesystemStorage::new(
            &data_dir,
            false,
            1024 * 1024,
            0,
            keyring,
            None,
            QuotaLimits::from_config(0, 0),
            false,
        )
        .await
        .unwrap();
        let config = Config {
            port: 0,
            address: "127.0.0.1".into(),
            data_dir,
            access_key: "k".into(),
            secret_key: "s".into(),
            region: "us-east-1".into(),
            master_key: None,
            allow_insecure_dev: true,
            secure_cookies: false,
            erasure_coding: false,
            chunk_size: 1024 * 1024,
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
            serve_ui: true,
            cluster_mode: true,
            storage_endpoints: String::new(),
            cluster_sync_interval_secs: 5,
            metrics_enabled: false,
            metrics_port: 0,
            audit_log: false,
            metadata_index: false,
            keycloak_enabled: false,
            keycloak_base_url: String::new(),
            keycloak_realm: "kubenexis".into(),
            keycloak_client_id: "maxio-ui".into(),
            keycloak_client_secret: None,
            keycloak_skip_tls_verify: false,
            keycloak_jwks_url: None,
            keycloak_issuer: None,
            keycloak_console_access_key: String::new(),
            default_tenant: "default".into(),
            allow_external_webhooks: false,
        };
        let config = Arc::new(config);
        let credentials = Arc::new(
            CredentialStore::load(&config.data_dir, &config)
                .await
                .unwrap(),
        );
        let state = new_app_state(
            dyn_storage(fs),
            config,
            Arc::new(crate::rate_limit::LoginRateLimiter::new()),
            credentials,
            None,
            None,
        );
        if let Some(cluster) = &state.cluster {
            cluster
                .publish(RoutingSnapshot {
                    epoch: 1,
                    storage_endpoints: vec![StorageEndpoint {
                        node_id: "1".into(),
                        addr: "storage-1:9100".into(),
                        is_leader: true,
                    }],
                    storage_quorum_ok: quorum_ok,
                    credential_epoch: 0,
                })
                .await;
        }
        state
    }

    #[tokio::test]
    async fn readyz_unavailable_without_storage_quorum() {
        let tmp = TempDir::new().unwrap();
        let state = cluster_app_state(&tmp, false).await;
        let app = build_router(state);
        let resp = app
            .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn colocated_single_node_defaults() {
        use clap::Parser;

        #[derive(Parser, Debug)]
        struct TestCli {
            #[command(flatten)]
            config: Config,
        }

        unsafe {
            std::env::remove_var("MAXIO_CLUSTER_MODE");
            std::env::remove_var("MAXIO_SERVE_UI");
        }
        let cli = TestCli::parse_from(["maxio", "--data-dir", "/data"]);
        assert!(!cli.config.cluster_mode);
        assert!(cli.config.serve_ui);
    }

    #[tokio::test]
    async fn create_bucket_proposes_to_storage_raft_leader() {
        use maxio_cluster::routing::parse_storage_peers;
        use maxio_cluster::{StorageRaftClient, wrap_cluster_storage};
        use maxio_storage::BucketMeta;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock = MockServer::start().await;
        let addr = mock.address();
        let status = maxio_cluster::StorageRaftStatus {
            node_id: 1,
            advertise_addr: addr.to_string(),
            current_leader: Some(1),
            is_leader: true,
            quorum_ok: true,
            commit_lag: 0,
        };

        Mock::given(method("GET"))
            .and(path("/internal/raft/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&status))
            .mount(&mock)
            .await;

        Mock::given(method("POST"))
            .and(path("/internal/raft/propose"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(maxio_cluster::storage::MutationResponse { ok: true }),
            )
            .expect(1)
            .mount(&mock)
            .await;

        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().to_str().unwrap().to_string();
        let keyring = Arc::new(Keyring::load(&data_dir, None).await.unwrap());
        let fs = FilesystemStorage::new(
            &data_dir,
            false,
            1024 * 1024,
            0,
            keyring,
            None,
            QuotaLimits::from_config(0, 0),
            false,
        )
        .await
        .unwrap();
        let inner = dyn_storage(fs);
        let peers = parse_storage_peers(&format!("1@{}", addr)).expect("parse mock storage peer");
        let cluster_storage = wrap_cluster_storage(inner, StorageRaftClient::new(peers));

        let created = cluster_storage
            .create_bucket(&BucketMeta {
                name: "raft-bucket".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
                region: "us-east-1".into(),
                versioning: false,
                cors_rules: None,
                encryption_config: None,
                public_read: false,
                public_list: false,
                bucket_policy: None,
                lifecycle_rules: None,
                erasure_coding: None,
                tenant_id: None,
                logging_target_bucket: None,
                logging_target_prefix: None,
                notification_config: None,
                object_lock_enabled: false,
                object_lock_config: None,
            })
            .await
            .expect("create_bucket");
        assert!(created);
        assert!(cluster_storage.head_bucket("raft-bucket").await.unwrap());
    }

    #[tokio::test]
    async fn readyz_ok_when_storage_quorum_healthy() {
        let tmp = TempDir::new().unwrap();
        let state = cluster_app_state(&tmp, true).await;
        let app = build_router(state);
        let resp = app
            .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let _body = resp.into_body().collect().await.unwrap();
    }
}
