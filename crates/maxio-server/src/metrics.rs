use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use http::header;

use crate::server::AppState;
use crate::storage::quota::disk_space_bytes;

const METHODS: [&str; 7] = ["GET", "PUT", "POST", "DELETE", "HEAD", "OPTIONS", "OTHER"];
const STATUS_CLASSES: [&str; 5] = ["2xx", "3xx", "4xx", "5xx", "other"];

/// Prometheus-style HTTP and storage gauges.
#[derive(Debug, Default)]
pub struct Metrics {
    requests: [[AtomicU64; STATUS_CLASSES.len()]; METHODS.len()],
    request_duration_sum_ns: AtomicU64,
    request_duration_count: AtomicU64,
    s3_slow_down_total: AtomicU64,
    upload_bytes_total: AtomicU64,
}

impl Metrics {
    pub fn record_request(&self, method: &Method, status: StatusCode, elapsed: Duration) {
        let method_idx = method_index(method);
        let status_idx = status_class_index(status);
        self.requests[method_idx][status_idx].fetch_add(1, Ordering::Relaxed);
        self.request_duration_sum_ns
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
        self.request_duration_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_slow_down(&self) {
        self.s3_slow_down_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_upload_bytes(&self, bytes: u64) {
        if bytes > 0 {
            self.upload_bytes_total.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    pub async fn render_prometheus(&self, state: &AppState) -> String {
        let mut out = String::new();

        out.push_str(
            "# HELP maxio_http_requests_total Total HTTP requests by method and status class\n",
        );
        out.push_str("# TYPE maxio_http_requests_total counter\n");
        for (mi, method) in METHODS.iter().enumerate() {
            for (si, class) in STATUS_CLASSES.iter().enumerate() {
                let value = self.requests[mi][si].load(Ordering::Relaxed);
                if value > 0 {
                    out.push_str(&format!(
                        "maxio_http_requests_total{{method=\"{method}\",status_class=\"{class}\"}} {value}\n"
                    ));
                }
            }
        }

        let sum_ns = self.request_duration_sum_ns.load(Ordering::Relaxed);
        let count = self.request_duration_count.load(Ordering::Relaxed);
        out.push_str(
            "# HELP maxio_http_request_duration_seconds_sum Sum of HTTP request durations\n",
        );
        out.push_str("# TYPE maxio_http_request_duration_seconds_sum counter\n");
        out.push_str(&format!(
            "maxio_http_request_duration_seconds_sum {}\n",
            sum_ns as f64 / 1_000_000_000.0
        ));
        out.push_str(
            "# HELP maxio_http_request_duration_seconds_count HTTP requests observed for latency\n",
        );
        out.push_str("# TYPE maxio_http_request_duration_seconds_count counter\n");
        out.push_str(&format!(
            "maxio_http_request_duration_seconds_count {count}\n"
        ));

        let slow = self.s3_slow_down_total.load(Ordering::Relaxed);
        out.push_str("# HELP maxio_s3_slow_down_total S3 rate-limit SlowDown responses\n");
        out.push_str("# TYPE maxio_s3_slow_down_total counter\n");
        out.push_str(&format!("maxio_s3_slow_down_total {slow}\n"));

        let uploaded = self.upload_bytes_total.load(Ordering::Relaxed);
        out.push_str("# HELP maxio_upload_bytes_total Bytes accepted on S3 PUT uploads\n");
        out.push_str("# TYPE maxio_upload_bytes_total counter\n");
        out.push_str(&format!("maxio_upload_bytes_total {uploaded}\n"));

        let uptime = state.started_at.elapsed().as_secs();
        out.push_str("# HELP maxio_uptime_seconds Process uptime\n");
        out.push_str("# TYPE maxio_uptime_seconds gauge\n");
        out.push_str(&format!("maxio_uptime_seconds {uptime}\n"));

        let multipart = state.storage.count_active_multipart_uploads().await;
        out.push_str("# HELP maxio_active_multipart_uploads Active multipart uploads\n");
        out.push_str("# TYPE maxio_active_multipart_uploads gauge\n");
        out.push_str(&format!("maxio_active_multipart_uploads {multipart}\n"));

        if let Some((total, free)) = disk_space_bytes(state.storage.data_root()) {
            out.push_str("# HELP maxio_disk_total_bytes Total bytes on data volume\n");
            out.push_str("# TYPE maxio_disk_total_bytes gauge\n");
            out.push_str(&format!("maxio_disk_total_bytes {total}\n"));
            out.push_str("# HELP maxio_disk_free_bytes Free bytes on data volume\n");
            out.push_str("# TYPE maxio_disk_free_bytes gauge\n");
            out.push_str(&format!("maxio_disk_free_bytes {free}\n"));
        }

        out
    }
}

pub async fn metrics_handler(State(state): State<AppState>) -> Response {
    let body = state.metrics.render_prometheus(&state).await;
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

pub async fn metrics_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let method = request.method().clone();
    let start = std::time::Instant::now();
    let response = next.run(request).await;
    if state.config.metrics_enabled {
        state
            .metrics
            .record_request(&method, response.status(), start.elapsed());
    }
    response
}

fn method_index(method: &Method) -> usize {
    match *method {
        Method::GET => 0,
        Method::PUT => 1,
        Method::POST => 2,
        Method::DELETE => 3,
        Method::HEAD => 4,
        Method::OPTIONS => 5,
        _ => 6,
    }
}

fn status_class_index(status: StatusCode) -> usize {
    match status.as_u16() {
        200..=299 => 0,
        300..=399 => 1,
        400..=499 => 2,
        500..=599 => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_and_status_indexes_are_stable() {
        assert_eq!(method_index(&Method::PUT), 1);
        assert_eq!(status_class_index(StatusCode::TOO_MANY_REQUESTS), 2);
    }

    #[test]
    fn metrics_counters_increment() {
        let metrics = Metrics::default();
        metrics.record_request(&Method::GET, StatusCode::OK, Duration::from_millis(5));
        metrics.record_slow_down();
        assert_eq!(
            metrics.requests[0][0].load(Ordering::Relaxed),
            1,
            "GET 2xx counter"
        );
        assert_eq!(metrics.s3_slow_down_total.load(Ordering::Relaxed), 1);
    }
}
