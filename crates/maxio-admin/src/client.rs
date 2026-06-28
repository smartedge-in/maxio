use crate::config::Profile;
use crate::error::{AdminError, Result};
use base64::Engine;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

const ADMIN_PREFIX: &str = "/api/admin/v1";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

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
        if let Some(token) = &self.profile.admin_token
            && !token.is_empty()
            && let Ok(v) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
        {
            headers.insert(reqwest::header::AUTHORIZATION, v);
            return headers;
        }
        if let (Some(access), Some(secret)) = (&self.profile.access_key, &self.profile.secret_key)
            && !access.is_empty()
            && !secret.is_empty()
        {
            let encoded = B64.encode(format!("{access}:{secret}"));
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&format!("Basic {encoded}")) {
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
                    "admin API not implemented".into()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_takes_precedence_over_basic() {
        let profile = Profile {
            endpoint: "http://127.0.0.1:9000".into(),
            region: None,
            access_key: Some("ak".into()),
            secret_key: Some("sk".into()),
            admin_token: Some("tok".into()),
            timeout_ms: None,
            tls_insecure: false,
        };
        let session = AdminSession {
            http: Client::new(),
            base: profile.endpoint.clone(),
            profile,
        };
        let headers = session.auth_headers();
        let auth = headers
            .get(reqwest::header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth.starts_with("Bearer tok"));
    }

    #[test]
    fn falls_back_to_basic_when_no_admin_token() {
        let profile = Profile {
            endpoint: "http://127.0.0.1:9000".into(),
            region: None,
            access_key: Some("ak".into()),
            secret_key: Some("sk".into()),
            admin_token: None,
            timeout_ms: None,
            tls_insecure: false,
        };
        let session = AdminSession {
            http: Client::new(),
            base: profile.endpoint.clone(),
            profile,
        };
        let headers = session.auth_headers();
        let auth = headers
            .get(reqwest::header::AUTHORIZATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth.starts_with("Basic "));
    }
}
