use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;

static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Initializes and installs the global Prometheus metrics recorder.
///
/// Idempotent across multiple calls / tests within the same process.
pub fn init_prometheus() -> PrometheusHandle {
    PROMETHEUS_HANDLE
        .get_or_init(|| {
            PrometheusBuilder::new()
                .install_recorder()
                .expect("failed to install Prometheus recorder")
        })
        .clone()
}

/// Creates a dedicated Axum router for the Prometheus metrics listener.
///
/// Binds solely to `ATIHSRAH_METRICS_BIND` and is strictly isolated from the public traffic router.
pub fn create_metrics_router(
    handle: PrometheusHandle,
    state: Option<std::sync::Arc<crate::state::AppState>>,
) -> Router {
    Router::new().route(
        "/metrics",
        get(move || {
            let h = handle.clone();
            let s = state.clone();
            async move {
                if let Some(ref st) = s {
                    st.sync_cache_gauges();
                }
                h.render().into_response()
            }
        }),
    )
}

// -----------------------------------------------------------------------------
// Metric Recording Helpers
// -----------------------------------------------------------------------------

/// Records a completed HTTP request with status class ("2xx", "4xx", "5xx")
/// and cache result ("hit", "miss", "none").
pub fn record_request(cache_result: &'static str, status_code: u16, duration_secs: f64) {
    let status_class = match status_code {
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "unknown",
    };

    metrics::counter!(
        "atihsrah_requests_total",
        "cache_result" => cache_result,
        "status_class" => status_class
    )
    .increment(1);

    metrics::histogram!("atihsrah_request_duration_seconds").record(duration_secs);
}

/// Records source origin image fetch duration in seconds.
pub fn record_fetch_duration(secs: f64) {
    metrics::histogram!("atihsrah_fetch_duration_seconds").record(secs);
}

/// Records image decoding duration in seconds.
pub fn record_decode_duration(secs: f64) {
    metrics::histogram!("atihsrah_decode_duration_seconds").record(secs);
}

/// Records image transformation (resizing/cropping/fitting) duration in seconds.
pub fn record_transform_duration(secs: f64) {
    metrics::histogram!("atihsrah_transform_duration_seconds").record(secs);
}

/// Records image encoding duration in seconds.
pub fn record_encode_duration(secs: f64) {
    metrics::histogram!("atihsrah_encode_duration_seconds").record(secs);
}

/// Records an application error by its error type code.
pub fn record_error(error_type: &'static str) {
    metrics::counter!("atihsrah_errors_total", "type" => error_type).increment(1);
}

/// Records an SSRF blocking event.
pub fn record_ssrf_blocked() {
    metrics::counter!("atihsrah_ssrf_blocked_total").increment(1);
    record_error("ssrf_blocked");
}

/// Records a rate limit 429 event.
pub fn record_rate_limited() {
    metrics::counter!("atihsrah_rate_limited_total").increment(1);
    record_error("rate_limited");
}

/// Records a signature failure event.
pub fn record_signature_invalid() {
    metrics::counter!("atihsrah_signature_invalid_total").increment(1);
    record_error("signature_invalid");
}

/// Records an image that was too large for single-entry cache insertion.
pub fn record_oversize_not_cached() {
    metrics::counter!("atihsrah_oversize_not_cached_total").increment(1);
}

/// Records when transformed output was larger than the source image.
pub fn record_output_larger_than_source() {
    metrics::counter!("atihsrah_output_larger_than_source_total").increment(1);
}

/// Updates gauges related to cache size, entry count, and hit ratios.
pub fn update_cache_gauges(size_bytes: u64, entries: u64, hit_ratio: f64, byte_hit_ratio: f64) {
    metrics::gauge!("atihsrah_cache_size_bytes").set(size_bytes as f64);
    metrics::gauge!("atihsrah_cache_entries").set(entries as f64);
    metrics::gauge!("atihsrah_cache_hit_ratio").set(hit_ratio);
    metrics::gauge!("atihsrah_cache_byte_hit_ratio").set(byte_hit_ratio);
}

/// Updates gauges related to concurrency and queue depths.
pub fn update_concurrency_gauges(
    inflight_requests: usize,
    inflight_transforms: usize,
    transform_queue_depth: usize,
) {
    metrics::gauge!("atihsrah_inflight_requests").set(inflight_requests as f64);
    metrics::gauge!("atihsrah_inflight_transforms").set(inflight_transforms as f64);
    metrics::gauge!("atihsrah_transform_queue_depth").set(transform_queue_depth as f64);
}
