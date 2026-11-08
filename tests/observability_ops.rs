use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use atihsrah::auth::{build_canonical_string, compute_signature, rfc3986_encode};
use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::SourceFetcher;
use atihsrah::http::create_router;
use atihsrah::observability::{create_metrics_router, init_prometheus, sha256_prefix};
use atihsrah::processor::image_rs::ImageRsProcessor;
use atihsrah::state::AppState;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
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
    rate_limit_per_minute: u32,
    rate_limit_miss_per_minute: u32,
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
        rate_limit_per_minute,
        rate_limit_miss_per_minute,
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
    let router = create_router(state.clone());
    (state, router, fetch_counter)
}

#[tokio::test]
async fn test_metrics_appear_and_move() {
    let prometheus_handle = init_prometheus();

    let raw_key =
        hex::decode("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef").unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), raw_key.clone());

    let (state, app, fetch_count) = test_setup(AuthMode::Signed, keys, 1000, 100);
    state.set_ready(true);

    let metrics_router = create_metrics_router(prometheus_handle.clone(), Some(state.clone()));

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now + 3600;

    // 1. Send first request -> Cache MISS
    let params1 = vec![
        ("exp".to_string(), exp.to_string()),
        ("format".to_string(), "webp".to_string()),
        (
            "src".to_string(),
            "https://images.example.com/obs-photo1.jpg".to_string(),
        ),
        ("w".to_string(), "50".to_string()),
    ];
    let canon1 = build_canonical_string(exp, &params1);
    let sig1 = compute_signature(&raw_key, &canon1);

    let uri1 = format!(
        "/transform?exp={}&format=webp&src=https%3A%2F%2Fimages.example.com%2Fobs-photo1.jpg&w=50&sig={}",
        exp, sig1
    );

    let mut req1 = Request::builder()
        .method("GET")
        .uri(&uri1)
        .body(Body::empty())
        .unwrap();
    req1.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(
        resp1
            .headers()
            .get("x-atihsrah-cache")
            .unwrap()
            .to_str()
            .unwrap(),
        "MISS"
    );
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);

    // 2. Send identical request -> Cache HIT
    let mut req2 = Request::builder()
        .method("GET")
        .uri(&uri1)
        .body(Body::empty())
        .unwrap();
    req2.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    assert_eq!(
        resp2
            .headers()
            .get("x-atihsrah-cache")
            .unwrap()
            .to_str()
            .unwrap(),
        "HIT"
    );
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1); // Unchanged

    // 3. Send invalid signature request -> 401 & signature_invalid_total
    let uri_invalid_sig = format!(
        "/transform?exp={}&format=webp&src=https%3A%2F%2Fimages.example.com%2Fobs-photo1.jpg&w=50&sig=deadbeef0000",
        exp
    );
    let mut req3 = Request::builder()
        .method("GET")
        .uri(&uri_invalid_sig)
        .body(Body::empty())
        .unwrap();
    req3.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));

    let resp3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::UNAUTHORIZED);

    // 4. Send disallowed origin request -> 403 & SSRF / SourceNotAllowed
    let params4 = vec![
        ("exp".to_string(), exp.to_string()),
        (
            "src".to_string(),
            "https://evil.notallowed.com/bad.jpg".to_string(),
        ),
    ];
    let canon4 = build_canonical_string(exp, &params4);
    let sig4 = compute_signature(&raw_key, &canon4);
    let uri4 = format!(
        "/transform?exp={}&src=https%3A%2F%2Fevil.notallowed.com%2Fbad.jpg&sig={}",
        exp, sig4
    );

    let mut req4 = Request::builder()
        .method("GET")
        .uri(&uri4)
        .body(Body::empty())
        .unwrap();
    req4.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));

    let resp4 = app.clone().oneshot(req4).await.unwrap();
    assert_eq!(resp4.status(), StatusCode::FORBIDDEN);

    // 5. Scrape Prometheus /metrics endpoint
    let metrics_req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let metrics_resp = metrics_router.oneshot(metrics_req).await.unwrap();
    assert_eq!(metrics_resp.status(), StatusCode::OK);

    let metrics_bytes = metrics_resp.into_body().collect().await.unwrap().to_bytes();
    let metrics_text = String::from_utf8(metrics_bytes.to_vec()).unwrap();

    // Verify key metrics exist and have moved
    assert!(
        metrics_text.contains("atihsrah_requests_total"),
        "Missing atihsrah_requests_total metric in output:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("cache_result=\"miss\""),
        "Missing cache_result=miss in atihsrah_requests_total:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("cache_result=\"hit\""),
        "Missing cache_result=hit in atihsrah_requests_total:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_request_duration_seconds"),
        "Missing atihsrah_request_duration_seconds histogram:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_fetch_duration_seconds"),
        "Missing atihsrah_fetch_duration_seconds histogram:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_decode_duration_seconds"),
        "Missing atihsrah_decode_duration_seconds histogram:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_transform_duration_seconds"),
        "Missing atihsrah_transform_duration_seconds histogram:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_encode_duration_seconds"),
        "Missing atihsrah_encode_duration_seconds histogram:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_cache_size_bytes"),
        "Missing atihsrah_cache_size_bytes gauge:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_cache_entries"),
        "Missing atihsrah_cache_entries gauge:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_cache_hit_ratio"),
        "Missing atihsrah_cache_hit_ratio gauge:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_cache_byte_hit_ratio"),
        "Missing atihsrah_cache_byte_hit_ratio gauge:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_inflight_requests"),
        "Missing atihsrah_inflight_requests gauge:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_signature_invalid_total"),
        "Missing atihsrah_signature_invalid_total counter:\n{metrics_text}"
    );
    assert!(
        metrics_text.contains("atihsrah_errors_total"),
        "Missing atihsrah_errors_total counter:\n{metrics_text}"
    );
}

#[tokio::test]
async fn test_liveness_vs_readiness_under_saturation() {
    let (state, app, _fc) = test_setup(AuthMode::Public, HashMap::new(), 1000, 100);

    // 1. Initially during startup: /health -> 200, /ready -> 503 (starting)
    let health_req1 = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let health_resp1 = app.clone().oneshot(health_req1).await.unwrap();
    assert_eq!(health_resp1.status(), StatusCode::OK);

    let ready_req1 = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    let ready_resp1 = app.clone().oneshot(ready_req1).await.unwrap();
    assert_eq!(ready_resp1.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body1 = ready_resp1.into_body().collect().await.unwrap().to_bytes();
    let json1: serde_json::Value = serde_json::from_slice(&body1).unwrap();
    assert_eq!(json1["status"], "starting");
    assert_eq!(json1["ready"], false);

    // 2. Mark ready: both /health and /ready -> 200
    state.set_ready(true);

    let health_req2 = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let health_resp2 = app.clone().oneshot(health_req2).await.unwrap();
    assert_eq!(health_resp2.status(), StatusCode::OK);

    let ready_req2 = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    let ready_resp2 = app.clone().oneshot(ready_req2).await.unwrap();
    assert_eq!(ready_resp2.status(), StatusCode::OK);
    let body2 = ready_resp2.into_body().collect().await.unwrap().to_bytes();
    let json2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert_eq!(json2["status"], "ready");

    // 3. Simulate queue saturation:
    // CRITICAL SPEC REQUIREMENT: High load or a full queue must NOT fail liveness!
    state.set_saturated_override(true);

    let health_req3 = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let health_resp3 = app.clone().oneshot(health_req3).await.unwrap();
    assert_eq!(
        health_resp3.status(),
        StatusCode::OK,
        "Liveness must remain 200 under saturation!"
    );

    let ready_req3 = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    let ready_resp3 = app.clone().oneshot(ready_req3).await.unwrap();
    assert_eq!(
        ready_resp3.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "Readiness must return 503 under saturation!"
    );
    let body3 = ready_resp3.into_body().collect().await.unwrap().to_bytes();
    let json3: serde_json::Value = serde_json::from_slice(&body3).unwrap();
    assert_eq!(json3["status"], "saturated");
    assert_eq!(json3["ready"], false);

    // Clear saturation: /ready returns to 200
    state.set_saturated_override(false);
    let ready_req3_clear = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    let ready_resp3_clear = app.clone().oneshot(ready_req3_clear).await.unwrap();
    assert_eq!(ready_resp3_clear.status(), StatusCode::OK);

    // 4. Simulate wedged runtime: /health must return 500
    state.set_wedged(true);

    let health_req4 = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let health_resp4 = app.clone().oneshot(health_req4).await.unwrap();
    assert_eq!(
        health_resp4.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "Liveness must fail (500) when wedged!"
    );
    let body4 = health_resp4.into_body().collect().await.unwrap().to_bytes();
    let json4: serde_json::Value = serde_json::from_slice(&body4).unwrap();
    assert_eq!(json4["status"], "wedged");
}

#[tokio::test]
async fn test_graceful_shutdown_draining() {
    let (state, app, _fc) = test_setup(AuthMode::Public, HashMap::new(), 1000, 100);
    state.set_ready(true);

    // Initially ready
    let ready_req1 = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    let ready_resp1 = app.clone().oneshot(ready_req1).await.unwrap();
    assert_eq!(ready_resp1.status(), StatusCode::OK);

    // Signal graceful drain (SIGTERM)
    state.start_draining();

    // /ready immediately flips to 503
    let ready_req2 = Request::builder()
        .method("GET")
        .uri("/ready")
        .body(Body::empty())
        .unwrap();
    let ready_resp2 = app.clone().oneshot(ready_req2).await.unwrap();
    assert_eq!(ready_resp2.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body2 = ready_resp2.into_body().collect().await.unwrap().to_bytes();
    let json2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert_eq!(json2["status"], "draining");
    assert_eq!(json2["ready"], false);

    // /health is still 200
    let health_req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let health_resp = app.clone().oneshot(health_req).await.unwrap();
    assert_eq!(health_resp.status(), StatusCode::OK);
}

#[derive(Clone, Default)]
struct SharedLogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for SharedLogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedLogBuffer {
    type Writer = SharedLogBuffer;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn test_log_redaction_secrets_and_urls_never_logged() {
    let log_buffer = SharedLogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(log_buffer.clone())
        .with_target(true)
        .finish();

    let _guard = tracing::subscriber::set_default(subscriber);

    let raw_key =
        hex::decode("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef").unwrap();
    let secret_key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), raw_key.clone());

    let (state, app, _fc) = test_setup(AuthMode::Signed, keys, 1000, 100);
    state.set_ready(true);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now + 3600;

    let sensitive_url =
        "https://images.example.com/user_profile_avatar_sensitive.png?token=supersecret123";
    let params = vec![
        ("exp".to_string(), exp.to_string()),
        ("format".to_string(), "webp".to_string()),
        ("src".to_string(), sensitive_url.to_string()),
        ("w".to_string(), "100".to_string()),
    ];
    let canon = build_canonical_string(exp, &params);
    let valid_sig = compute_signature(&raw_key, &canon);

    // Send successful signed request
    let uri = format!(
        "/transform?exp={}&format=webp&src={}&w=100&sig={}",
        exp,
        rfc3986_encode(sensitive_url),
        valid_sig
    );
    let mut req1 = Request::builder()
        .method("GET")
        .uri(&uri)
        .header(
            header::AUTHORIZATION,
            "Bearer confidential_user_jwt_token_9999",
        )
        .body(Body::empty())
        .unwrap();
    req1.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Send bad signature request (triggers security log)
    let bad_sig = "deadbeef99999999999999999999999999999999999999999999999999999999";
    let uri_bad = format!(
        "/transform?exp={}&format=webp&src={}&w=100&sig={}",
        exp,
        rfc3986_encode(sensitive_url),
        bad_sig
    );
    let mut req2 = Request::builder()
        .method("GET")
        .uri(&uri_bad)
        .body(Body::empty())
        .unwrap();
    req2.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::UNAUTHORIZED);

    // Send SSRF blocked request (triggers security log)
    let ssrf_url = "https://127.0.0.1/private-internal-admin";
    let params_ssrf = vec![
        ("exp".to_string(), exp.to_string()),
        ("src".to_string(), ssrf_url.to_string()),
    ];
    let canon_ssrf = build_canonical_string(exp, &params_ssrf);
    let sig_ssrf = compute_signature(&raw_key, &canon_ssrf);
    let uri_ssrf = format!(
        "/transform?exp={}&src={}&sig={}",
        exp,
        rfc3986_encode(ssrf_url),
        sig_ssrf
    );
    let mut req3 = Request::builder()
        .method("GET")
        .uri(&uri_ssrf)
        .body(Body::empty())
        .unwrap();
    req3.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));
    let resp3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::FORBIDDEN);

    // Inspect captured logs
    let raw_logs = String::from_utf8(log_buffer.0.lock().unwrap().clone()).unwrap();
    assert!(!raw_logs.is_empty(), "Logs must have captured events!");

    let expected_hash_prefix = sha256_prefix(sensitive_url);

    // Verify each line is valid JSON and contains timestamp, level, request_id
    for line in raw_logs.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("Failed to parse log line as JSON: {line} - {e}"));

        assert!(
            parsed.get("timestamp").is_some(),
            "Line missing timestamp: {line}"
        );
        assert!(parsed.get("level").is_some(), "Line missing level: {line}");
        let has_request_id = parsed.get("request_id").is_some()
            || parsed
                .get("fields")
                .and_then(|f| f.get("request_id"))
                .is_some();
        assert!(has_request_id, "Line missing request_id: {line}");
        let req_id_str = parsed
            .get("request_id")
            .or_else(|| parsed.get("fields").and_then(|f| f.get("request_id")))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            !req_id_str.is_empty() && req_id_str != "-",
            "Line has invalid request_id: {line}"
        );
    }

    // Check presence of targets: access and security
    assert!(
        raw_logs.contains(r#""target":"access""#),
        "Logs must contain target 'access':\n{raw_logs}"
    );
    assert!(
        raw_logs.contains(r#""target":"security""#),
        "Logs must contain target 'security':\n{raw_logs}"
    );

    // Redaction Checks:
    // 1. Raw URL must NEVER appear
    assert!(
        !raw_logs.contains(sensitive_url),
        "LEAK DETECTED: Raw source URL found in logs!"
    );
    assert!(
        !raw_logs.contains("supersecret123"),
        "LEAK DETECTED: Sensitive query token found in logs!"
    );
    assert!(
        !raw_logs.contains(ssrf_url),
        "LEAK DETECTED: SSRF target URL found in logs!"
    );

    // 2. Secret signing key must NEVER appear
    assert!(
        !raw_logs.contains(secret_key_hex),
        "LEAK DETECTED: Secret signing key found in logs!"
    );

    // 3. Raw signatures must NEVER appear
    assert!(
        !raw_logs.contains(&valid_sig),
        "LEAK DETECTED: Valid HMAC signature found in logs!"
    );
    assert!(
        !raw_logs.contains(bad_sig),
        "LEAK DETECTED: Invalid signature found in logs!"
    );

    // 4. Authorization headers must NEVER appear
    assert!(
        !raw_logs.contains("confidential_user_jwt_token_9999"),
        "LEAK DETECTED: Authorization header contents found in logs!"
    );

    // 5. Redacted SHA-256 prefix MUST appear
    assert!(
        raw_logs.contains(&expected_hash_prefix),
        "Redacted sha256 prefix {} must be present in logs:\n{raw_logs}",
        expected_hash_prefix
    );
}

#[tokio::test]
async fn test_security_headers_and_body_limits() {
    let (state, app, _fc) = test_setup(AuthMode::Public, HashMap::new(), 1000, 100);
    state.set_ready(true);

    // 1. Verify X-Content-Type-Options: nosniff
    let health_req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let health_resp = app.clone().oneshot(health_req).await.unwrap();
    assert_eq!(
        health_resp
            .headers()
            .get("x-content-type-options")
            .unwrap()
            .to_str()
            .unwrap(),
        "nosniff"
    );

    // 2. Test Content-Length exceeding max_body_bytes (65536) -> 413 Payload Too Large
    let oversize_body_len = 100_000;
    let oversize_req = Request::builder()
        .method("GET")
        .uri("/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg")
        .header(header::CONTENT_LENGTH, oversize_body_len.to_string())
        .body(Body::empty())
        .unwrap();

    let oversize_resp = app.clone().oneshot(oversize_req).await.unwrap();
    assert_eq!(oversize_resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body_bytes = oversize_resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["code"], "PAYLOAD_TOO_LARGE");

    // 3. Test excessive headers (>32KB) -> 431 Request Header Fields Too Large
    let mut big_req_builder = Request::builder().method("GET").uri("/health");
    let huge_header_val = "x".repeat(35000);
    big_req_builder = big_req_builder.header("x-huge-header", huge_header_val);
    let big_req = big_req_builder.body(Body::empty()).unwrap();

    let big_resp = app.clone().oneshot(big_req).await.unwrap();
    assert_eq!(
        big_resp.status(),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
    );
    let err_bytes = big_resp.into_body().collect().await.unwrap().to_bytes();
    let err_json: serde_json::Value = serde_json::from_slice(&err_bytes).unwrap();
    assert_eq!(err_json["code"], "HEADERS_TOO_LARGE");
}
