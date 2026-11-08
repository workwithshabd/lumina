use atihsrah::config::{AuthMode, Config};
use atihsrah::http::{create_router, X_REQUEST_ID};
use atihsrah::processor::stub::StubImageProcessor;
use atihsrah::state::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::collections::HashSet;
use std::sync::Arc;
use tower::ServiceExt;

fn test_app_state() -> (Arc<AppState>, axum::Router) {
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
        auth_mode: AuthMode::Public,
        signing_keys: std::collections::HashMap::new(),
        signing_key: None,
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: 1000,
        rate_limit_miss_per_minute: 100,
        trusted_proxy_cidrs: Vec::new(),
        log_level: "info".to_string(),
        debug_headers: false,
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

    let processor = Arc::new(StubImageProcessor::new());
    let state = Arc::new(AppState::new(config, processor).unwrap());
    let router = create_router(state.clone());
    (state, router)
}

#[tokio::test]
async fn test_health_endpoint() {
    let (_state, app) = test_app_state();

    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req_id_header = resp.headers().get(&X_REQUEST_ID);
    assert!(req_id_header.is_some());
    let req_id = req_id_header.unwrap().to_str().unwrap();
    assert_eq!(req_id.len(), 26); // standard ULID length

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_ready_endpoint_lifecycle() {
    let (state, app) = test_app_state();

    // 1. Initially not ready -> returns 503
    let req1 = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body1 = resp1.into_body().collect().await.unwrap().to_bytes();
    let json1: serde_json::Value = serde_json::from_slice(&body1).unwrap();
    assert_eq!(json1["status"], "starting");

    // 2. Mark ready -> returns 200
    state.set_ready(true);

    let req2 = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();

    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    let body2 = resp2.into_body().collect().await.unwrap().to_bytes();
    let json2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert_eq!(json2["status"], "ready");
}

#[tokio::test]
async fn test_request_id_custom_propagation() {
    let (_state, app) = test_app_state();

    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .header("x-request-id", "client-trace-12345")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req_id = resp.headers().get(&X_REQUEST_ID).unwrap().to_str().unwrap();
    assert_eq!(req_id, "client-trace-12345");
}

#[tokio::test]
async fn test_request_id_malformed_falls_back_to_ulid() {
    let (_state, app) = test_app_state();

    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .header("x-request-id", "invalid req id with spaces and @#$")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req_id = resp.headers().get(&X_REQUEST_ID).unwrap().to_str().unwrap();
    assert_eq!(req_id.len(), 26); // Replaced with valid ULID
}
