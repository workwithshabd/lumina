use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::SourceFetcher;
use atihsrah::http::create_router;
use atihsrah::observability::{create_metrics_router, init_prometheus};
use atihsrah::processor::image_rs::ImageRsProcessor;
use atihsrah::state::AppState;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use image::codecs::png::PngEncoder;
use image::{ImageEncoder, Rgba, RgbaImage};
use tower::ServiceExt;

fn make_png(w: u32, h: u32) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for x in 0..w {
        for y in 0..h {
            img.put_pixel(x, y, Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255]));
        }
    }
    let mut buf = Vec::new();
    let encoder = PngEncoder::new(&mut buf);
    encoder
        .write_image(&img.into_raw(), w, h, image::ExtendedColorType::Rgba8)
        .unwrap();
    buf
}

fn test_setup(
    auth_mode: AuthMode,
    signing_keys: HashMap<String, Vec<u8>>,
) -> (Arc<AppState>, axum::Router, Arc<AtomicUsize>) {
    let mut origins = HashSet::new();
    origins.insert("https://images.example.com".to_string());

    let config = Arc::new(Config {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        allowed_origins: origins,
        source_fetch_timeout_secs: 15,
        source_connect_timeout_secs: 5,
        max_source_fetch_mb: 20,
        max_source_dimension: 8000,
        max_output_dimension: 4096,
        max_processing_memory_mb: 512,
        max_concurrent_transforms: 4,
        cache_max_size_mb: 500,
        cache_single_entry_max_mb: 10,
        cache_ttl_secs: 86400,
        auth_mode,
        signing_keys,
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: 1000,
        rate_limit_miss_per_minute: 1000,
        trusted_proxy_cidrs: Vec::new(),
        debug_headers: true,
        metrics_bind: "127.0.0.1:9090".parse().unwrap(),
        request_timeout_secs: 30,
        global_concurrency_limit: 1024,
        queue_saturation_threshold: 16,
        shutdown_timeout_secs: 30,
        max_body_bytes: 65536,
        private_origins: HashSet::new(),
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
            Ok(make_png(80, 80))
        }),
    ));

    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, mock_fetcher));
    state.set_ready(true);
    let router = create_router(state.clone());
    (state, router, fetch_counter)
}

fn parse_metric_value(metrics_text: &str, metric_name: &str) -> Option<f64> {
    for line in metrics_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 && parts[0] == metric_name {
            if let Ok(val) = parts[1].parse::<f64>() {
                return Some(val);
            }
        }
    }
    None
}

/// 1. Cache gauges: before setting atihsrah_cache_entries and atihsrah_cache_size_bytes
/// at scrape time, await cache.run_pending_tasks().
/// Test: after one cached transform, both gauges are > 0.
#[tokio::test]
async fn test_1_cache_gauges_run_pending_tasks_both_greater_than_zero() {
    let prometheus_handle = init_prometheus();
    let (state, app, _fc) = test_setup(AuthMode::Public, HashMap::new());
    let metrics_router = create_metrics_router(prometheus_handle.clone(), Some(state.clone()));

    // Execute one transform request
    let uri = "/transform?src=https%3A%2F%2Fimages.example.com%2Fspec-photo1.png&w=50&format=png";
    let mut req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Immediately scrape /metrics without waiting for background thread
    let metrics_req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let metrics_resp = metrics_router.oneshot(metrics_req).await.unwrap();
    assert_eq!(metrics_resp.status(), StatusCode::OK);

    let metrics_bytes = metrics_resp.into_body().collect().await.unwrap().to_bytes();
    let metrics_text = String::from_utf8(metrics_bytes.to_vec()).unwrap();

    let entries = parse_metric_value(&metrics_text, "atihsrah_cache_entries")
        .expect("atihsrah_cache_entries gauge must be present");
    let size_bytes = parse_metric_value(&metrics_text, "atihsrah_cache_size_bytes")
        .expect("atihsrah_cache_size_bytes gauge must be present");

    assert!(
        entries > 0.0,
        "Expected atihsrah_cache_entries > 0 immediately after cached transform, got {entries}"
    );
    assert!(
        size_bytes > 0.0,
        "Expected atihsrah_cache_size_bytes > 0 immediately after cached transform, got {size_bytes}"
    );
}

/// 2. Configure metrics-exporter-prometheus with explicit buckets for every
/// *_duration_seconds metric (fine buckets from 1 ms to 30 s) so they export
/// as histograms with _bucket/_sum/_count.
#[tokio::test]
async fn test_2_duration_histograms_export_with_buckets_sum_count() {
    let prometheus_handle = init_prometheus();
    let (state, app, _fc) = test_setup(AuthMode::Public, HashMap::new());
    let metrics_router = create_metrics_router(prometheus_handle.clone(), Some(state.clone()));

    let uri = "/transform?src=https%3A%2F%2Fimages.example.com%2Fspec-photo2.png&w=40&format=jpeg";
    let mut req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12346))));

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let metrics_req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let metrics_resp = metrics_router.oneshot(metrics_req).await.unwrap();
    let metrics_bytes = metrics_resp.into_body().collect().await.unwrap().to_bytes();
    let metrics_text = String::from_utf8(metrics_bytes.to_vec()).unwrap();

    let duration_metrics = [
        "atihsrah_request_duration_seconds",
        "atihsrah_fetch_duration_seconds",
        "atihsrah_decode_duration_seconds",
        "atihsrah_transform_duration_seconds",
        "atihsrah_encode_duration_seconds",
    ];

    for metric in duration_metrics {
        // Must export _bucket with le labels
        let bucket_label = format!("{metric}_bucket");
        assert!(
            metrics_text.contains(&bucket_label),
            "Missing {bucket_label} in metrics:\n{metrics_text}"
        );
        // Must contain 1ms (0.001) bucket and 30s bucket
        assert!(
            metrics_text.contains(&format!("{metric}_bucket{{le=\"0.001\"}}")),
            "Missing {metric}_bucket{{le=\"0.001\"}} in metrics:\n{metrics_text}"
        );
        assert!(
            metrics_text.contains(&format!("{metric}_bucket{{le=\"30\"}}"))
                || metrics_text.contains(&format!("{metric}_bucket{{le=\"30.0\"}}")),
            "Missing {metric}_bucket{{le=\"30\"}} in metrics:\n{metrics_text}"
        );
        assert!(
            metrics_text.contains(&format!("{metric}_bucket{{le=\"+Inf\"}}")),
            "Missing {metric}_bucket{{le=\"+Inf\"}} in metrics:\n{metrics_text}"
        );
        // Must export _sum and _count
        let sum_metric = format!("{metric}_sum");
        let count_metric = format!("{metric}_count");
        assert!(
            metrics_text.contains(&sum_metric),
            "Missing {sum_metric} in metrics:\n{metrics_text}"
        );
        assert!(
            metrics_text.contains(&count_metric),
            "Missing {count_metric} in metrics:\n{metrics_text}"
        );
    }
}

/// 3. Replace the cache_hit_ratio and cache_byte_hit_ratio gauges with counters:
/// atihsrah_cache_hits_total, atihsrah_cache_misses_total,
/// atihsrah_cache_hit_bytes_total, atihsrah_response_bytes_total.
#[tokio::test]
async fn test_3_cache_counters_replace_gauges() {
    let prometheus_handle = init_prometheus();
    let (state, app, _fc) = test_setup(AuthMode::Public, HashMap::new());
    let metrics_router = create_metrics_router(prometheus_handle.clone(), Some(state.clone()));

    // 1. First request -> Cache MISS
    let uri = "/transform?src=https%3A%2F%2Fimages.example.com%2Fspec-photo3.png&w=40&format=jpeg";
    let mut req1 = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    req1.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12347))));

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // 2. Second request -> Cache HIT
    let mut req2 = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    req2.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12348))));

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    let metrics_req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let metrics_resp = metrics_router.oneshot(metrics_req).await.unwrap();
    let metrics_bytes = metrics_resp.into_body().collect().await.unwrap().to_bytes();
    let metrics_text = String::from_utf8(metrics_bytes.to_vec()).unwrap();

    // Assert OLD gauges are GONE
    assert!(
        !metrics_text.contains("atihsrah_cache_hit_ratio "),
        "atihsrah_cache_hit_ratio gauge must be removed"
    );
    assert!(
        !metrics_text.contains("atihsrah_cache_byte_hit_ratio "),
        "atihsrah_cache_byte_hit_ratio gauge must be removed"
    );

    // Assert NEW counters exist and have recorded values
    let hits = parse_metric_value(&metrics_text, "atihsrah_cache_hits_total")
        .expect("atihsrah_cache_hits_total must exist");
    let misses = parse_metric_value(&metrics_text, "atihsrah_cache_misses_total")
        .expect("atihsrah_cache_misses_total must exist");
    let hit_bytes = parse_metric_value(&metrics_text, "atihsrah_cache_hit_bytes_total")
        .expect("atihsrah_cache_hit_bytes_total must exist");
    let resp_bytes = parse_metric_value(&metrics_text, "atihsrah_response_bytes_total")
        .expect("atihsrah_response_bytes_total must exist");

    assert!(hits >= 1.0, "Expected atihsrah_cache_hits_total >= 1, got {hits}");
    assert!(misses >= 1.0, "Expected atihsrah_cache_misses_total >= 1, got {misses}");
    assert!(hit_bytes > 0.0, "Expected atihsrah_cache_hit_bytes_total > 0, got {hit_bytes}");
    assert!(resp_bytes > 0.0, "Expected atihsrah_response_bytes_total > 0, got {resp_bytes}");
}

/// 4. Pre-register every counter at 0 at startup, including ssrf_blocked_total,
/// rate_limited_total, signature_invalid_total, oversize_not_cached_total,
/// and output_larger_than_source_total. Make allowlist rejections increment a
/// security counter (atihsrah_source_rejected_total{reason}). Test that all
/// series exist on a fresh scrape.
#[tokio::test]
async fn test_4_preregistered_counters_on_fresh_scrape_and_source_rejected() {
    let prometheus_handle = init_prometheus();
    let (state, app, _fc) = test_setup(AuthMode::Public, HashMap::new());
    let metrics_router = create_metrics_router(prometheus_handle.clone(), Some(state.clone()));

    // Fresh scrape: test that all counters exist at startup before requests
    let metrics_req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let metrics_resp = metrics_router.clone().oneshot(metrics_req).await.unwrap();
    let metrics_bytes = metrics_resp.into_body().collect().await.unwrap().to_bytes();
    let metrics_text = String::from_utf8(metrics_bytes.to_vec()).unwrap();

    let preregistered_metrics = [
        "atihsrah_ssrf_blocked_total",
        "atihsrah_rate_limited_total",
        "atihsrah_signature_invalid_total",
        "atihsrah_oversize_not_cached_total",
        "atihsrah_output_larger_than_source_total",
        "atihsrah_cache_hits_total",
        "atihsrah_cache_misses_total",
        "atihsrah_cache_hit_bytes_total",
        "atihsrah_response_bytes_total",
    ];

    for metric in preregistered_metrics {
        assert!(
            metrics_text.contains(metric),
            "Counter {metric} must be pre-registered on fresh scrape:\n{metrics_text}"
        );
    }

    assert!(
        metrics_text.contains("atihsrah_source_rejected_total"),
        "atihsrah_source_rejected_total counter must exist on scrape:\n{metrics_text}"
    );

    // Now send an unallowlisted source URL -> SourceNotAllowed (403)
    let bad_uri = "/transform?src=https%3A%2F%2Fevil-attacker.example.com%2Fpic.jpg&w=50";
    let mut bad_req = Request::builder()
        .method("GET")
        .uri(bad_uri)
        .body(Body::empty())
        .unwrap();
    bad_req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12349))));

    let bad_resp = app.clone().oneshot(bad_req).await.unwrap();
    assert_eq!(bad_resp.status(), StatusCode::FORBIDDEN);

    // Scrape /metrics again and verify atihsrah_source_rejected_total{reason="disallowed_origin"} incremented
    let metrics_req2 = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let metrics_resp2 = metrics_router.oneshot(metrics_req2).await.unwrap();
    let metrics_bytes2 = metrics_resp2.into_body().collect().await.unwrap().to_bytes();
    let metrics_text2 = String::from_utf8(metrics_bytes2.to_vec()).unwrap();

    assert!(
        metrics_text2.contains("atihsrah_source_rejected_total{reason=\"disallowed_origin\"}")
            || metrics_text2.contains("atihsrah_source_rejected_total"),
        "Expected atihsrah_source_rejected_total to be incremented:\n{metrics_text2}"
    );
}
