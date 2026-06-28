use thiserror::Error;

pub type Result<T> = std::result::Result<T, AdminError>;

#[derive(Debug, Error)]
pub enum AdminError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("profile not found: {0}")]
    ProfileNotFound(String),

    #[error("admin API not available at {url}: {message}")]
    ApiNotAvailable { url: String, message: String },

    #[error("admin API returned HTTP {status}: {body}")]
    ApiHttp { status: u16, body: String },

    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("{0}")]
    Stub(String),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
