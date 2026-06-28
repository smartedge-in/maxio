use clap::Args;
use std::env;

fn first_env_value(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| env::var(key).ok().filter(|value| !value.trim().is_empty()))
}

fn default_access_key() -> String {
    first_env_value(&["MINIO_ROOT_USER", "MINIO_ACCESS_KEY"])
        .unwrap_or_else(|| "maxioadmin".to_string())
}

fn default_secret_key() -> String {
    first_env_value(&["MINIO_ROOT_PASSWORD", "MINIO_SECRET_KEY"])
        .unwrap_or_else(|| "maxioadmin".to_string())
}

fn default_region() -> String {
    first_env_value(&["MINIO_REGION_NAME", "MINIO_REGION"])
        .unwrap_or_else(|| "us-east-1".to_string())
}

fn default_default_buckets() -> Option<String> {
    first_env_value(&["MINIO_DEFAULT_BUCKETS"])
}

#[derive(Args, Debug, Clone)]
pub struct Config {
    /// Port to listen on
    #[arg(long, env = "MAXIO_PORT", default_value = "9000")]
    pub port: u16,

    /// Address to bind to
    #[arg(long, env = "MAXIO_ADDRESS", default_value = "0.0.0.0")]
    pub address: String,

    /// Root data directory
    #[arg(long, env = "MAXIO_DATA_DIR", default_value = "./data")]
    pub data_dir: String,

    /// Access key (MAXIO_ACCESS_KEY, MINIO_ROOT_USER, MINIO_ACCESS_KEY)
    #[arg(long, env = "MAXIO_ACCESS_KEY", default_value_t = default_access_key())]
    pub access_key: String,

    /// Secret key (MAXIO_SECRET_KEY, MINIO_ROOT_PASSWORD, MINIO_SECRET_KEY)
    #[arg(long, env = "MAXIO_SECRET_KEY", default_value_t = default_secret_key())]
    pub secret_key: String,

    /// Default region (MAXIO_REGION, MINIO_REGION_NAME, MINIO_REGION)
    #[arg(long, env = "MAXIO_REGION", default_value_t = default_region())]
    pub region: String,

    /// Master key for SSE-S3 encryption (base64-encoded 32 bytes).
    /// When set, takes precedence over the keyring file for new writes.
    #[arg(long, env = "MAXIO_MASTER_KEY")]
    pub master_key: Option<String>,

    /// Allow insecure development defaults (default credentials, HTTP cookies).
    #[arg(long, env = "MAXIO_ALLOW_INSECURE_DEV", default_value = "false")]
    pub allow_insecure_dev: bool,

    /// Force Secure on console session cookies. Keep enabled for public consoles.
    #[arg(long, env = "MAXIO_SECURE_COOKIES", default_value = "true")]
    pub secure_cookies: bool,

    /// Enable erasure coding with per-chunk integrity checksums
    #[arg(long, env = "MAXIO_ERASURE_CODING", default_value = "false")]
    pub erasure_coding: bool,

    /// Chunk size in bytes for erasure coding (default 10MB)
    #[arg(long, env = "MAXIO_CHUNK_SIZE", default_value = "10485760")]
    pub chunk_size: u64,

    /// Number of parity shards for erasure coding (0 = no parity, requires --erasure-coding)
    #[arg(long, env = "MAXIO_PARITY_SHARDS", default_value = "0")]
    pub parity_shards: u32,

    /// Comma-separated list of bucket names to create on first boot
    /// (MAXIO_DEFAULT_BUCKETS, MINIO_DEFAULT_BUCKETS)
    #[arg(long, env = "MAXIO_DEFAULT_BUCKETS", default_value_t = default_default_buckets().unwrap_or_default())]
    pub default_buckets: String,

    /// Max request body size for console JSON/form API routes, in bytes. Object uploads are streaming and not covered by this limit.
    #[arg(long, env = "MAXIO_MAX_CONSOLE_BODY_BYTES", default_value = "1048576")]
    pub max_console_body_bytes: usize,

    /// Maximum S3 object size in bytes (0 = unlimited).
    #[arg(long, env = "MAXIO_MAX_OBJECT_BYTES", default_value = "0")]
    pub max_object_bytes: u64,

    /// Minimum free disk bytes to reserve on the data volume (0 = disabled).
    #[arg(long, env = "MAXIO_MIN_FREE_DISK_BYTES", default_value = "0")]
    pub min_free_disk_bytes: u64,

    /// Max failed S3 auth attempts per client IP per window (0 = disabled).
    #[arg(long, env = "MAXIO_S3_RATE_AUTH_MAX", default_value = "60")]
    pub s3_rate_auth_max: u32,

    /// Sliding window for S3 auth failure rate limit, in seconds.
    #[arg(long, env = "MAXIO_S3_RATE_AUTH_WINDOW_SECS", default_value = "300")]
    pub s3_rate_auth_window_secs: u64,

    /// Max S3 PUT requests per client IP per window (0 = disabled).
    #[arg(long, env = "MAXIO_S3_RATE_PUT_MAX", default_value = "0")]
    pub s3_rate_put_max: u32,

    /// Sliding window for S3 PUT rate limit, in seconds.
    #[arg(long, env = "MAXIO_S3_RATE_PUT_WINDOW_SECS", default_value = "60")]
    pub s3_rate_put_window_secs: u64,

    /// Bearer token for `/api/admin/v1` (empty = Bearer auth disabled; Basic access/secret still accepted).
    #[arg(long, env = "MAXIO_ADMIN_TOKEN", default_value = "")]
    pub admin_token: String,

    /// Max admin API requests per client IP per window (0 = disabled).
    #[arg(long, env = "MAXIO_ADMIN_RATE_MAX", default_value = "120")]
    pub admin_rate_max: u32,

    /// Sliding window for admin API rate limit, in seconds.
    #[arg(long, env = "MAXIO_ADMIN_RATE_WINDOW_SECS", default_value = "60")]
    pub admin_rate_window_secs: u64,

    /// Comma-separated trusted proxy CIDRs. When the direct peer matches, `X-Forwarded-For` is used for client IP (console login + rate limits).
    #[arg(long, env = "MAXIO_TRUSTED_PROXIES", default_value = "")]
    pub trusted_proxies: String,

    /// Optional Redis URL for distributed console login rate limiting across replicas (`redis://host:6379`).
    #[arg(long, env = "MAXIO_LOGIN_RATE_LIMIT_REDIS_URL")]
    pub login_rate_limit_redis_url: Option<String>,

    /// Public S3 endpoint host for virtual-hosted-style requests (`bucket.{server_host}`), e.g. `s3.example.com` or `localhost:9000`.
    #[arg(long, env = "MAXIO_SERVER_HOST", default_value = "")]
    pub server_host: String,
}

#[cfg(test)]
mod tests {
    use super::Config;
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(flatten)]
        config: Config,
    }

    #[test]
    fn default_address_is_all_interfaces() {
        unsafe {
            std::env::remove_var("MAXIO_ADDRESS");
        }

        let cli = TestCli::parse_from(["maxio"]);

        assert_eq!(cli.config.address, "0.0.0.0");
    }
}
