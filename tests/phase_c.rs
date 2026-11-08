use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use atihsrah::config::{check_unknown_env_vars, AuthMode, Config};
use atihsrah::fetch::SourceFetcher;
use atihsrah::observability::metrics::{create_metrics_router, init_prometheus};
use atihsrah::processor::stub::StubImageProcessor;
use atihsrah::state::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_setup(
    debug_headers: bool,
) -> (Arc<AppState>, axum::Router, axum::Router, Arc<AtomicUsize>) {
    let mut allowed = std::collections::HashSet::new();
    allowed.insert("https://images.unsplash.com".to_string());

    let config = Arc::new(Config {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        allowed_origins: allowed,
        source_fetch_timeout_secs: 10,
        source_connect_timeout_secs: 2,
        max_source_fetch_mb: 20,
        max_source_dimension: 8000,
        max_output_dimension: 4096,
        max_processing_memory_mb: 512,
        max_concurrent_transforms: 4,
        cache_max_size_mb: 500,
        cache_single_entry_max_mb: 10,
        cache_ttl_secs: 86400,
        auth_mode: AuthMode::Public,
        signing_keys: HashMap::new(),
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: 10000,
        rate_limit_miss_per_minute: 1000,
        trusted_proxy_cidrs: Vec::new(),
        debug_headers,
        metrics_bind: "127.0.0.1:9090".parse().unwrap(),
        request_timeout_secs: 30,
        global_concurrency_limit: 1024,
        queue_saturation_threshold: 16,
        shutdown_timeout_secs: 5,
        max_body_bytes: 65536,
        private_origins: std::collections::HashSet::new(),
        private_ca_file: None,
        avif_speed: 6,
        distinct_color_threshold: 256,
        max_retry_pixels: 16_777_216,
    });

    let fetch_counter = Arc::new(AtomicUsize::new(0));
    let fc = fetch_counter.clone();

    let mock_fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| {
            fc.fetch_add(1, Ordering::SeqCst);
            // Minimal 1x1 PNG bytes
            let png_bytes = vec![
                0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
                0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
                0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78,
                0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
                0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
            ];
            Ok(png_bytes)
        }),
    ));

    let processor = Arc::new(StubImageProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(
        config.clone(),
        processor,
        mock_fetcher,
    ));
    let app = atihsrah::http::create_router(state.clone());
    let handle = init_prometheus();
    let metrics_router = create_metrics_router(handle, Some(state.clone()));

    (state, app, metrics_router, fetch_counter)
}

// ---------------------------------------------------------------------------
// 1. Unknown Config Warnings & Closest Match Suggestions
// ---------------------------------------------------------------------------
#[test]
fn test_unknown_config_warnings_and_suggestions() {
    let test_env = vec![
        "ATIHSRAH_DEBUG_HEADER", // Typo of ATIHSRAH_DEBUG_HEADERS
        "ATIHSRAH_SIGNING_KYE",  // Typo of ATIHSRAH_SIGNING_KEY
        "ALLOWED_ORIGIN",        // Typo of ALLOWED_ORIGINS
        "CACHE_MAX_SIZE",        // Typo of CACHE_MAX_SIZE_MB
        "PATH",                  // Unrelated standard system var
        "HOME",                  // Unrelated standard system var
        "USER",                  // Unrelated standard system var
    ];

    let warnings = check_unknown_env_vars(&test_env);

    let map: HashMap<String, String> = warnings.into_iter().collect();

    // Verify typos of ATIHSRAH_* trigger warnings with exact closest suggestions
    assert_eq!(
        map.get("ATIHSRAH_DEBUG_HEADER").map(|s| s.as_str()),
        Some("ATIHSRAH_DEBUG_HEADERS")
    );
    assert_eq!(
        map.get("ATIHSRAH_SIGNING_KYE").map(|s| s.as_str()),
        Some("ATIHSRAH_SIGNING_KEY")
    );

    // Verify documented non-prefixed typos trigger warnings
    assert_eq!(
        map.get("ALLOWED_ORIGIN").map(|s| s.as_str()),
        Some("ALLOWED_ORIGINS")
    );
    assert_eq!(
        map.get("CACHE_MAX_SIZE").map(|s| s.as_str()),
        Some("CACHE_MAX_SIZE_MB")
    );

    // Verify unrelated system variables are NOT falsely flagged as Atihsrah configurations
    assert!(!map.contains_key("PATH"));
    assert!(!map.contains_key("HOME"));
    assert!(!map.contains_key("USER"));
}

// ---------------------------------------------------------------------------
// 2. Metrics prefixed with atihsrah_
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_metrics_prefixed_with_atihsrah() {
    let (_state, app, metrics_app, _fc) = test_setup(false);

    // Execute one request to increment metrics
    let req = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/test.jpg&w=100")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Fetch /metrics
    let m_req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let m_resp = metrics_app.oneshot(m_req).await.unwrap();
    assert_eq!(m_resp.status(), StatusCode::OK);

    let bytes = m_resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();

    // Assert that metric names now strictly contain the atihsrah_ prefix
    assert!(
        text.contains("atihsrah_requests_total"),
        "Expected 'atihsrah_requests_total' in metrics output:\n{text}"
    );
    assert!(
        text.contains("atihsrah_request_duration_seconds"),
        "Expected 'atihsrah_request_duration_seconds' in metrics output:\n{text}"
    );
    assert!(
        text.contains("atihsrah_cache_size_bytes"),
        "Expected 'atihsrah_cache_size_bytes' in metrics output:\n{text}"
    );
    assert!(
        text.contains("atihsrah_cache_entries"),
        "Expected 'atihsrah_cache_entries' in metrics output:\n{text}"
    );
    assert!(
        text.contains("atihsrah_cache_hit_ratio"),
        "Expected 'atihsrah_cache_hit_ratio' in metrics output:\n{text}"
    );
    assert!(
        text.contains("atihsrah_cache_byte_hit_ratio"),
        "Expected 'atihsrah_cache_byte_hit_ratio' in metrics output:\n{text}"
    );

    // Assert that unprefixed metrics do NOT appear
    for line in text.lines() {
        if line.starts_with("# TYPE requests_total") || line.starts_with("requests_total{") {
            panic!("Found unprefixed metric line: {line}");
        }
        if line.starts_with("# TYPE cache_size_bytes") || line.starts_with("cache_size_bytes ") {
            panic!("Found unprefixed gauge line: {line}");
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Debug Headers: appear ONLY when ATIHSRAH_DEBUG_HEADERS=true
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_debug_headers_appear_only_when_enabled() {
    let url = "/transform?src=https://images.unsplash.com/test.jpg&w=100";

    // Subtest A: With ATIHSRAH_DEBUG_HEADERS=false
    let (_state_off, app_off, _m, _fc) = test_setup(false);
    let req_off = Request::builder()
        .method("GET")
        .uri(url)
        .body(Body::empty())
        .unwrap();
    let resp_off = app_off.oneshot(req_off).await.unwrap();
    assert_eq!(resp_off.status(), StatusCode::OK);

    // x-atihsrah-cache MUST still be present (cache hit/miss indicator is standard)
    assert!(resp_off.headers().contains_key("x-atihsrah-cache"));
    // Measured timing debug headers MUST NOT be present
    assert!(
        !resp_off.headers().contains_key("x-atihsrah-fetch-ms"),
        "x-atihsrah-fetch-ms must not appear when debug_headers=false"
    );
    assert!(
        !resp_off.headers().contains_key("x-atihsrah-process-ms"),
        "x-atihsrah-process-ms must not appear when debug_headers=false"
    );
    assert!(
        !resp_off.headers().contains_key("x-atihsrah-total-ms"),
        "x-atihsrah-total-ms must not appear when debug_headers=false"
    );

    // Subtest B: With ATIHSRAH_DEBUG_HEADERS=true
    let (_state_on, app_on, _m, _fc) = test_setup(true);
    let req_on = Request::builder()
        .method("GET")
        .uri(url)
        .body(Body::empty())
        .unwrap();
    let resp_on = app_on.clone().oneshot(req_on).await.unwrap();
    assert_eq!(resp_on.status(), StatusCode::OK);

    assert!(resp_on.headers().contains_key("x-atihsrah-cache"));
    assert_eq!(resp_on.headers().get("x-atihsrah-cache").unwrap(), "MISS");
    // On cache miss, all three timing headers MUST be present
    assert!(
        resp_on.headers().contains_key("x-atihsrah-fetch-ms"),
        "x-atihsrah-fetch-ms must appear when debug_headers=true on cache miss"
    );
    assert!(
        resp_on.headers().contains_key("x-atihsrah-process-ms"),
        "x-atihsrah-process-ms must appear when debug_headers=true on cache miss"
    );
    assert!(
        resp_on.headers().contains_key("x-atihsrah-total-ms"),
        "x-atihsrah-total-ms must appear when debug_headers=true"
    );

    // Second request is a cache HIT
    let req_hit = Request::builder()
        .method("GET")
        .uri(url)
        .body(Body::empty())
        .unwrap();
    let resp_hit = app_on.oneshot(req_hit).await.unwrap();
    assert_eq!(resp_hit.status(), StatusCode::OK);
    assert_eq!(resp_hit.headers().get("x-atihsrah-cache").unwrap(), "HIT");

    // On cache hit, fetch and process times are absent (no origin fetch/processing took place)
    assert!(!resp_hit.headers().contains_key("x-atihsrah-fetch-ms"));
    assert!(!resp_hit.headers().contains_key("x-atihsrah-process-ms"));
    // Total time is present
    assert!(resp_hit.headers().contains_key("x-atihsrah-total-ms"));

    let total_str = resp_hit
        .headers()
        .get("x-atihsrah-total-ms")
        .unwrap()
        .to_str()
        .unwrap();
    let total_val: u64 = total_str.parse().expect("total_ms must be valid integer");
    // Sub-millisecond cache hits round honestly to 0ms
    assert!(
        total_val <= 50,
        "Cache hit total time must be small, got: {total_val}ms"
    );
}
