use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug)]
struct Bucket {
    count: u32,
    window_start: Instant,
}

/// In-memory sliding-window rate limiter keyed by client identifier (typically IP).
#[derive(Debug)]
pub struct SlidingWindowLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
    max: u32,
    window_secs: u64,
}

impl SlidingWindowLimiter {
    /// `max == 0` disables the limiter (always allows).
    pub fn new(max: u32, window_secs: u64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max,
            window_secs,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.max > 0
    }

    /// Increments the counter for `key`. Returns `Some(retry_after_secs)` when over limit.
    pub fn try_acquire(&self, key: &str) -> Option<u64> {
        if !self.is_enabled() {
            return None;
        }

        let mut map = self.buckets.lock().unwrap();
        let now = Instant::now();
        let window = self.window_secs;

        map.retain(|_, bucket| now.duration_since(bucket.window_start).as_secs() < window * 2);

        let bucket = map.entry(key.to_string()).or_insert(Bucket {
            count: 0,
            window_start: now,
        });

        if now.duration_since(bucket.window_start).as_secs() >= window {
            bucket.count = 0;
            bucket.window_start = now;
        }

        bucket.count += 1;

        if bucket.count > self.max {
            let remaining = window.saturating_sub(now.duration_since(bucket.window_start).as_secs());
            Some(remaining.max(1))
        } else {
            None
        }
    }
}

enum LoginRateLimiterBackend {
    Memory(SlidingWindowLimiter),
    Redis {
        manager: redis::aio::ConnectionManager,
        max: u32,
        window_secs: u64,
    },
}

/// Console login rate limiter (counts every attempt, success or failure).
///
/// Uses an in-memory sliding window by default. Set `MAXIO_LOGIN_RATE_LIMIT_REDIS_URL`
/// for a shared store when running multiple console replicas.
pub struct LoginRateLimiter {
    backend: LoginRateLimiterBackend,
}

impl LoginRateLimiter {
    const DEFAULT_MAX: u32 = 10;
    const DEFAULT_WINDOW_SECS: u64 = 300;

    pub fn new() -> Self {
        Self::in_memory(Self::DEFAULT_MAX, Self::DEFAULT_WINDOW_SECS)
    }

    pub fn in_memory(max: u32, window_secs: u64) -> Self {
        Self {
            backend: LoginRateLimiterBackend::Memory(SlidingWindowLimiter::new(
                max, window_secs,
            )),
        }
    }

    pub async fn from_config(redis_url: Option<&str>) -> anyhow::Result<Self> {
        if let Some(url) = redis_url.filter(|s| !s.trim().is_empty()) {
            let client = redis::Client::open(url.to_string())?;
            let manager = client.get_connection_manager().await?;
            tracing::info!("console login rate limiter using Redis backend");
            return Ok(Self {
                backend: LoginRateLimiterBackend::Redis {
                    manager,
                    max: Self::DEFAULT_MAX,
                    window_secs: Self::DEFAULT_WINDOW_SECS,
                },
            });
        }
        Ok(Self::new())
    }

    /// Returns `Some(retry_after_secs)` if the IP is rate-limited, `None` if allowed.
    pub async fn check_and_increment(&self, ip: &str) -> Option<u64> {
        match &self.backend {
            LoginRateLimiterBackend::Memory(inner) => inner.try_acquire(ip),
            LoginRateLimiterBackend::Redis {
                manager,
                max,
                window_secs,
            } => redis_check_and_increment(manager.clone(), ip, *max, *window_secs).await,
        }
    }
}

async fn redis_check_and_increment(
    mut manager: redis::aio::ConnectionManager,
    ip: &str,
    max: u32,
    window_secs: u64,
) -> Option<u64> {
    if max == 0 {
        return None;
    }
    let key = format!("maxio:login:{ip}");
    let count: u32 = match redis::cmd("INCR")
        .arg(&key)
        .query_async(&mut manager)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("redis login rate limit INCR failed: {e}");
            return None;
        }
    };
    if count == 1 {
        let _: Result<(), _> = redis::cmd("EXPIRE")
            .arg(&key)
            .arg(window_secs)
            .query_async(&mut manager)
            .await;
    }
    if count > max {
        let ttl: i64 = redis::cmd("TTL")
            .arg(&key)
            .query_async(&mut manager)
            .await
            .unwrap_or(1);
        Some(ttl.max(1) as u64)
    } else {
        None
    }
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct S3RateLimiter {
    pub auth_failures: SlidingWindowLimiter,
    pub put_requests: SlidingWindowLimiter,
}

impl S3RateLimiter {
    pub fn from_config(
        auth_max: u32,
        auth_window_secs: u64,
        put_max: u32,
        put_window_secs: u64,
    ) -> Self {
        Self {
            auth_failures: SlidingWindowLimiter::new(auth_max, auth_window_secs),
            put_requests: SlidingWindowLimiter::new(put_max, put_window_secs),
        }
    }
}

/// Admin API rate limiter (counts every authenticated request).
pub struct AdminRateLimiter {
    inner: SlidingWindowLimiter,
}

impl AdminRateLimiter {
    pub fn from_config(max: u32, window_secs: u64) -> Self {
        Self {
            inner: SlidingWindowLimiter::new(max, window_secs),
        }
    }

    /// Returns `Some(retry_after_secs)` if the IP is rate-limited, `None` if allowed.
    pub fn check_and_increment(&self, ip: &str) -> Option<u64> {
        self.inner.try_acquire(ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_limiter_never_blocks() {
        let limiter = SlidingWindowLimiter::new(0, 60);
        for _ in 0..100 {
            assert!(limiter.try_acquire("127.0.0.1").is_none());
        }
    }

    #[test]
    fn limiter_blocks_after_max_requests() {
        let limiter = SlidingWindowLimiter::new(3, 60);
        assert!(limiter.try_acquire("10.0.0.1").is_none());
        assert!(limiter.try_acquire("10.0.0.1").is_none());
        assert!(limiter.try_acquire("10.0.0.1").is_none());
        let retry = limiter.try_acquire("10.0.0.1");
        assert!(retry.is_some());
        assert!(retry.unwrap() >= 1);
    }

    #[test]
    fn limiter_tracks_keys_independently() {
        let limiter = SlidingWindowLimiter::new(1, 60);
        assert!(limiter.try_acquire("a").is_none());
        assert!(limiter.try_acquire("b").is_none());
        assert!(limiter.try_acquire("a").is_some());
        assert!(limiter.try_acquire("b").is_some());
    }

    #[tokio::test]
    async fn login_limiter_memory_backend_blocks() {
        let limiter = LoginRateLimiter::in_memory(2, 60);
        assert!(limiter.check_and_increment("1.2.3.4").await.is_none());
        assert!(limiter.check_and_increment("1.2.3.4").await.is_none());
        assert!(limiter.check_and_increment("1.2.3.4").await.is_some());
    }
}