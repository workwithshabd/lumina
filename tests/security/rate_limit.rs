use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::SourceFetcher;
use atihsrah::http::create_router;
use atihsrah::processor::image_rs::ImageRsProcessor;
use atihsrah::state::AppState;
use reqwest::StatusCode;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_rate_limit_bypass_spoofed_x_forwarded_for_ignored() {
    // Server configured with strict rate limit (3 per minute) and NO trusted proxies
    let mut origins = HashSet::new();
    origins.insert("https://images.example.com".to_string());

    let config = Arc::new(Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        allowed_origins: origins,
        source_fetch_timeout_secs: 5,
        source_connect_timeout_secs: 2,
        max_source_fetch_mb: 20,
        max_source_dimension: 8000,
        max_output_dimension: 4096,
        max_processing_memory_mb: 512,
        max_concurrent_transforms: 4,
        cache_max_size_mb: 50,
        cache_single_entry_max_mb: 5,
        cache_ttl_secs: 3600,
        auth_mode: AuthMode::Public,
        signing_keys: HashMap::new(),
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: 3,
        rate_limit_miss_per_minute: 3,
        trusted_proxy_cidrs: Vec::new(), // Untrusted!
        debug_headers: true,
        metrics_bind: "127.0.0.1:0".parse().unwrap(),
        request_timeout_secs: 10,
        global_concurrency_limit: 512,
        queue_saturation_threshold: 16,
        shutdown_timeout_secs: 5,
        max_body_bytes: 65536,
        private_origins: HashSet::new(),
        private_ca_file: None,
        avif_speed: 6,
        distinct_color_threshold: 256,
        max_retry_pixels: 16_777_216,
        resize_filter: "lanczos3".to_string(),
    });

    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(|_url: &str| Ok(super::fixtures::make_valid_png(32, 32))),
    ));
    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    state.set_ready(true);

    let router = create_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let client = reqwest::Client::new();

    // Attacker rotates X-Forwarded-For with fake IPs to bypass rate limiter
    for i in 1..=3 {
        let uri = format!(
            "http://{addr}/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=100&v={i}"
        );
        let resp = client
            .get(&uri)
            .header("X-Forwarded-For", format!("203.0.113.{i}"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Request #{i} within quota should succeed"
        );
    }

    // 4th request from same socket connection with another spoofed IP must be REJECTED (429)
    // because X-Forwarded-For is untrusted and rate limiting keys on peer IP (127.0.0.1)
    let uri4 = format!(
        "http://{addr}/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=100&v=99"
    );
    let resp4 = client
        .get(&uri4)
        .header("X-Forwarded-For", "198.51.100.99")
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp4.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "Spoofed X-Forwarded-For must NOT bypass rate limiter when proxy is untrusted"
    );

    assert!(
        resp4.headers().contains_key("retry-after"),
        "429 response must contain Retry-After header"
    );
}
