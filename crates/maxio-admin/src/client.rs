use crate::config::Profile;
use crate::error::{AdminError, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

const ADMIN_PREFIX: &str = "/api/admin/v1";

/// HTTP session for the MaxIO admin API (P2-13).
pub struct AdminSession {
    http: Client,
    base: String,
    profile: Profile,
}

impl AdminSession {
    pub fn connect(profile: Profile) -> Result<Self> {
        let timeout = Duration::from_millis(profile.timeout_ms.unwrap_or(10_000));
        let mut builder = Client::builder().timeout(timeout);
        if profile.tls_insecure {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let http = builder
            .build()
            .map_err(|e| AdminError::Config(format!("HTTP client: {e}")))?;

        let base = profile.endpoint.trim_end_matches('/').to_string();
        Ok(Self {
            http,
            base,
            profile,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.base
    }

    pub async fn status(&self) -> Result<Value> {
        self.get("/status").await
    }

    pub async fn info(&self) -> Result<Value> {
        self.get("/info").await
    }

    pub async fn doctor(&self) -> Result<Value> {
        self.get("/doctor").await
    }

    pub async fn list_buckets(&self) -> Result<Value> {
        self.get("/buckets").await
    }

    pub async fn head_bucket(&self, name: &str) -> Result<Value> {
        self.get(&format!("/buckets/{name}")).await
    }

    pub async fn keyring_list(&self) -> Result<Value> {
        self.get("/keyring").await
    }

    pub async fn housekeeping_run(&self) -> Result<Value> {
        self.post("/housekeeping/run").await
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{ADMIN_PREFIX}{path}");
        let resp = self
            .http
            .get(format!("{}{url}", self.base))
            .headers(self.auth_headers())
            .send()
            .await?;
        self.decode(resp).await
    }

    async fn post(&self, path: &str) -> Result<Value> {
        let url = format!("{ADMIN_PREFIX}{path}");
        let resp = self
            .http
            .post(format!("{}{url}", self.base))
            .headers(self.auth_headers())
            .send()
            .await?;
        self.decode(resp).await
    }

    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        // Stub: P2-13 will define the canonical admin auth scheme.
        if let Some(token) = &self.profile.admin_token {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
                headers.insert(reqwest::header::AUTHORIZATION, v);
            }
        }
        headers
    }

    async fn decode<T: DeserializeOwned>(&self, resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        let url = resp.url().to_string();
        let body = resp.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::NOT_IMPLEMENTED {
            return Err(AdminError::ApiNotAvailable {
                url,
                message: if body.is_empty() {
                    "admin API not implemented yet (P2-13)".into()
                } else {
                    body
                },
            });
        }
        if !status.is_success() {
            return Err(AdminError::ApiHttp {
                status: status.as_u16(),
                body,
            });
        }
        serde_json::from_str(&body).map_err(|e| AdminError::ApiNotAvailable {
            url,
            message: format!("invalid JSON: {e}; body={body}"),
        })
    }
}