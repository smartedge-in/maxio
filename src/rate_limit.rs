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

pub fn client_ip_from_request(request: &axum::extract::Request) -> String {
    request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|connect_info| connect_info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
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

/// Console login rate limiter (counts every attempt, success or failure).
pub struct LoginRateLimiter {
    inner: SlidingWindowLimiter,
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: SlidingWindowLimiter::new(10, 300),
        }
    }

    /// Returns `Some(retry_after_secs)` if the IP is rate-limited, `None` if allowed.
    pub fn check_and_increment(&self, ip: &str) -> Option<u64> {
        self.inner.try_acquire(ip)
    }
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new()
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
}