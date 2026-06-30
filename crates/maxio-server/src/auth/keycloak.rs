//! Native Keycloak OIDC integration for the web console (password + refresh grants).

use crate::config::Config;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// HttpOnly cookie storing the Keycloak refresh token for the console.
pub const REFRESH_COOKIE_NAME: &str = "maxio_kc_refresh";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Keycloak token endpoint response (OpenAPI-compatible with Kubenexis services).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct KeycloakTokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    pub refresh_expires_in: i64,
    pub refresh_token: String,
    pub token_type: String,
    pub scope: String,
}

/// Public discovery payload for the console UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeycloakConfigResponse {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KeycloakSettings {
    pub base_url: String,
    pub realm: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub skip_tls_verify: bool,
    pub jwks_url: Option<String>,
    pub issuer: Option<String>,
}

impl KeycloakSettings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            base_url: config.keycloak_base_url.trim_end_matches('/').to_string(),
            realm: config.keycloak_realm.clone(),
            client_id: config.keycloak_client_id.clone(),
            client_secret: config.keycloak_client_secret.clone(),
            skip_tls_verify: config.keycloak_skip_tls_verify,
            jwks_url: config.keycloak_jwks_url.clone(),
            issuer: config.keycloak_issuer.clone(),
        }
    }

    pub fn issuer_url(&self) -> String {
        self.issuer
            .clone()
            .unwrap_or_else(|| format!("{}/realms/{}", self.base_url, self.realm))
    }

    pub fn jwks_uri(&self) -> String {
        self.jwks_url
            .clone()
            .unwrap_or_else(|| format!("{}/protocol/openid-connect/certs", self.issuer_url()))
    }

    pub fn token_url(&self) -> String {
        format!("{}/protocol/openid-connect/token", self.issuer_url())
    }

    pub fn config_response(&self) -> KeycloakConfigResponse {
        KeycloakConfigResponse {
            enabled: true,
            realm: Some(self.realm.clone()),
            client_id: Some(self.client_id.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeycloakClaims {
    pub subject: String,
    pub preferred_username: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum KeycloakError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("keycloak unreachable: {0}")]
    Unreachable(String),
    #[error("keycloak token error: {0}")]
    TokenEndpoint(String),
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("keycloak is not configured")]
    NotConfigured,
}

pub struct KeycloakAuth {
    settings: KeycloakSettings,
    http: Client,
    jwks: RwLock<JwksCache>,
}

struct JwksCache {
    keys: HashMap<String, DecodingKey>,
    fetched_at: Option<Instant>,
}

impl KeycloakAuth {
    pub fn from_config(config: &Config) -> anyhow::Result<Self> {
        config.validate_keycloak()?;
        if !config.keycloak_enabled {
            anyhow::bail!("Keycloak is not enabled");
        }
        Ok(Self::new(KeycloakSettings::from_config(config)))
    }

    pub fn new(settings: KeycloakSettings) -> Self {
        let mut builder = Client::builder().timeout(HTTP_TIMEOUT);
        if settings.skip_tls_verify {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let http = builder
            .build()
            .expect("reqwest client should build with default features");

        Self {
            settings,
            http,
            jwks: RwLock::new(JwksCache {
                keys: HashMap::new(),
                fetched_at: None,
            }),
        }
    }

    pub fn settings(&self) -> &KeycloakSettings {
        &self.settings
    }

    pub fn refresh_cookie_name(&self) -> &'static str {
        REFRESH_COOKIE_NAME
    }

    pub async fn password_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<KeycloakTokenResponse, KeycloakError> {
        let mut form = vec![
            ("grant_type", "password"),
            ("client_id", self.settings.client_id.as_str()),
            ("username", username),
            ("password", password),
        ];
        let secret;
        if let Some(ref s) = self.settings.client_secret {
            secret = s.as_str();
            form.push(("client_secret", secret));
        }
        self.post_token(form).await
    }

    pub async fn refresh(
        &self,
        refresh_token: &str,
    ) -> Result<KeycloakTokenResponse, KeycloakError> {
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("client_id", self.settings.client_id.as_str()),
            ("refresh_token", refresh_token),
        ];
        let secret;
        if let Some(ref s) = self.settings.client_secret {
            secret = s.as_str();
            form.push(("client_secret", secret));
        }
        self.post_token(form).await
    }

    async fn post_token(
        &self,
        form: Vec<(&str, &str)>,
    ) -> Result<KeycloakTokenResponse, KeycloakError> {
        let response = self
            .http
            .post(self.settings.token_url())
            .header("content-type", "application/x-www-form-urlencoded")
            .form(&form)
            .send()
            .await
            .map_err(|e| KeycloakError::Unreachable(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| KeycloakError::Unreachable(e.to_string()))?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(KeycloakError::InvalidCredentials);
        }
        if !status.is_success() {
            return Err(KeycloakError::TokenEndpoint(body.trim().to_string()));
        }

        serde_json::from_str(&body)
            .map_err(|e| KeycloakError::TokenEndpoint(format!("decode response: {e}")))
    }

    pub async fn validate_access_token(
        &self,
        token: &str,
    ) -> Result<KeycloakClaims, KeycloakError> {
        if token.is_empty() {
            return Err(KeycloakError::InvalidToken("empty token".into()));
        }

        let header = decode_header(token)
            .map_err(|e| KeycloakError::InvalidToken(format!("header: {e}")))?;
        let kid = header
            .kid
            .ok_or_else(|| KeycloakError::InvalidToken("missing kid".into()))?;

        let decoding_key = self.decoding_key_for_kid(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.settings.issuer_url()]);
        validation.validate_exp = true;

        let token_data = decode::<serde_json::Value>(token, &decoding_key, &validation)
            .map_err(|e| KeycloakError::InvalidToken(e.to_string()))?;

        let claims = &token_data.claims;
        if !self.client_authorized(claims) {
            return Err(KeycloakError::InvalidToken(
                "token not issued for this client".into(),
            ));
        }

        let subject = claims
            .get("sub")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let preferred_username = claims
            .get("preferred_username")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        Ok(KeycloakClaims {
            subject,
            preferred_username,
        })
    }

    fn client_authorized(&self, claims: &serde_json::Value) -> bool {
        let client_id = self.settings.client_id.as_str();
        if claims
            .get("azp")
            .and_then(|v| v.as_str())
            .is_some_and(|azp| azp == client_id)
        {
            return true;
        }
        match claims.get("aud") {
            Some(serde_json::Value::String(aud)) => aud == client_id,
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str())
                .any(|aud| aud == client_id),
            _ => false,
        }
    }

    async fn decoding_key_for_kid(&self, kid: &str) -> Result<DecodingKey, KeycloakError> {
        {
            let cache = self.jwks.read().expect("jwks lock poisoned");
            if let Some(key) = cache.keys.get(kid) {
                return Ok(key.clone());
            }
        }
        self.refresh_jwks().await?;
        let cache = self.jwks.read().expect("jwks lock poisoned");
        cache
            .keys
            .get(kid)
            .cloned()
            .ok_or_else(|| KeycloakError::InvalidToken(format!("unknown kid {kid}")))
    }

    async fn refresh_jwks(&self) -> Result<(), KeycloakError> {
        let response = self
            .http
            .get(self.settings.jwks_uri())
            .send()
            .await
            .map_err(|e| KeycloakError::Unreachable(format!("fetch jwks: {e}")))?;

        if !response.status().is_success() {
            return Err(KeycloakError::Unreachable(format!(
                "jwks status {}",
                response.status()
            )));
        }

        let body: JwksResponse = response
            .json()
            .await
            .map_err(|e| KeycloakError::Unreachable(format!("decode jwks: {e}")))?;

        let mut keys = HashMap::new();
        for key in body.keys {
            if key.kty != "RSA" {
                continue;
            }
            if let Ok(decoding_key) = DecodingKey::from_rsa_components(&key.n, &key.e) {
                keys.insert(key.kid, decoding_key);
            }
        }

        let mut cache = self.jwks.write().expect("jwks lock poisoned");
        cache.keys = keys;
        cache.fetched_at = Some(Instant::now());
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct JwksResponse {
    keys: Vec<JwkKey>,
}

#[derive(Debug, Deserialize)]
struct JwkKey {
    kid: String,
    kty: String,
    n: String,
    e: String,
}

/// Returns true when the cookie value is a legacy HMAC console session (not a JWT).
pub fn is_legacy_console_session(token: &str) -> bool {
    let mut parts = token.split('.');
    let Some(issued_hex) = parts.next() else {
        return false;
    };
    let Some(_sig) = parts.next() else {
        return false;
    };
    let Some(fp) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    issued_hex.chars().all(|c| c.is_ascii_hexdigit())
        && !issued_hex.is_empty()
        && fp.len() == 8
        && fp.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_settings(base_url: &str) -> KeycloakSettings {
        KeycloakSettings {
            base_url: base_url.to_string(),
            realm: "kubenexis".to_string(),
            client_id: "maxio-ui".to_string(),
            client_secret: None,
            skip_tls_verify: false,
            jwks_url: None,
            issuer: None,
        }
    }

    #[test]
    fn legacy_session_detection() {
        let now = chrono::Utc::now().timestamp();
        let issued_hex = format!("{now:x}");
        assert!(is_legacy_console_session(&format!(
            "{issued_hex}.deadbeef.cafebabe"
        )));
        assert!(!is_legacy_console_session(
            "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxIn0.sig"
        ));
    }

    #[tokio::test]
    async fn password_login_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/realms/kubenexis/protocol/openid-connect/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "access-1",
                "expires_in": 300,
                "refresh_expires_in": 1800,
                "refresh_token": "refresh-1",
                "token_type": "Bearer",
                "scope": "openid profile"
            })))
            .mount(&server)
            .await;

        let mut settings = test_settings(&server.uri());
        settings.issuer = Some(format!("{}/realms/kubenexis", server.uri()));
        let auth = KeycloakAuth::new(settings);

        let tokens = auth.password_login("alice", "secret").await.unwrap();
        assert_eq!(tokens.access_token, "access-1");
        assert_eq!(tokens.refresh_token, "refresh-1");
    }

    #[tokio::test]
    async fn password_login_invalid_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/realms/kubenexis/protocol/openid-connect/token"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid_grant"))
            .mount(&server)
            .await;

        let mut settings = test_settings(&server.uri());
        settings.issuer = Some(format!("{}/realms/kubenexis", server.uri()));
        let auth = KeycloakAuth::new(settings);

        let err = auth.password_login("alice", "wrong").await.unwrap_err();
        assert!(matches!(err, KeycloakError::InvalidCredentials));
    }

    #[test]
    fn config_validate_requires_base_url() {
        let config = Config {
            port: 9000,
            address: "127.0.0.1".into(),
            data_dir: "./data".into(),
            access_key: "k".into(),
            secret_key: "s".into(),
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
            serve_ui: true,
            cluster_mode: false,
            storage_endpoints: String::new(),
            cluster_sync_interval_secs: 5,
            metrics_enabled: false,
            metrics_port: 0,
            audit_log: false,
            metadata_index: false,
            keycloak_enabled: true,
            keycloak_base_url: String::new(),
            keycloak_realm: "kubenexis".into(),
            keycloak_client_id: "maxio-ui".into(),
            keycloak_client_secret: None,
            keycloak_skip_tls_verify: false,
            keycloak_jwks_url: None,
            keycloak_issuer: None,
            default_tenant: "default".into(),
            allow_external_webhooks: false,
        };
        assert!(config.validate_keycloak().is_err());
    }
}
