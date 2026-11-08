use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use atihsrah::auth::{build_canonical_string, compute_signature};
use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::SourceFetcher;
use atihsrah::http::create_router;
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
    rate_limit_per_minute: u32,
    rate_limit_miss_per_minute: u32,
    trusted_proxy_cidrs: Vec<ipnet::IpNet>,
) -> (Arc<AppState>, axum::Router) {
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
        trusted_proxy_cidrs,
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
    let mock_fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| {
            fetch_counter.fetch_add(1, Ordering::SeqCst);
            Ok(make_png(80, 80))
        }),
    ));

    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, mock_fetcher));
    state.set_ready(true);
    let router = create_router(state.clone());
    (state, router)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// -----------------------------------------------------------------------------
// Signed URLs Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn test_valid_signed_url_succeeds() {
    let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key_bytes = hex::decode(key_hex).unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

    let (_state, app) = test_setup(AuthMode::Signed, keys, 1000, 100, Vec::new());

    let exp = now_secs() + 3600;
    let params = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo.jpg".to_string(),
        ),
        ("w".to_string(), "100".to_string()),
        ("exp".to_string(), exp.to_string()),
    ];
    let canonical = build_canonical_string(exp, &params);
    let sig = compute_signature(&key_bytes, &canonical);

    let uri = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=100&exp={exp}&sig={sig}"
    );

    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_tampered_param_returns_401() {
    let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key_bytes = hex::decode(key_hex).unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

    let (_state, app) = test_setup(AuthMode::Signed, keys, 1000, 100, Vec::new());

    let exp = now_secs() + 3600;
    let params = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo.jpg".to_string(),
        ),
        ("w".to_string(), "100".to_string()),
        ("exp".to_string(), exp.to_string()),
    ];
    let canonical = build_canonical_string(exp, &params);
    let sig = compute_signature(&key_bytes, &canonical);

    // Tampered: change w from 100 to 200
    let uri = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=200&exp={exp}&sig={sig}"
    );

    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_reordered_params_verifies_successfully() {
    let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key_bytes = hex::decode(key_hex).unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

    let (_state, app) = test_setup(AuthMode::Signed, keys, 1000, 100, Vec::new());

    let exp = now_secs() + 3600;
    let params = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo.jpg".to_string(),
        ),
        ("w".to_string(), "100".to_string()),
        ("exp".to_string(), exp.to_string()),
    ];
    let canonical = build_canonical_string(exp, &params);
    let sig = compute_signature(&key_bytes, &canonical);

    // Reordered in query: exp, w, src, sig
    let uri = format!(
        "/transform?exp={exp}&w=100&src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&sig={sig}"
    );

    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_duplicated_param_rejected_with_422() {
    let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key_bytes = hex::decode(key_hex).unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

    let (_state, app) = test_setup(AuthMode::Signed, keys, 1000, 100, Vec::new());

    let exp = now_secs() + 3600;
    let uri = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=100&w=200&exp={exp}&sig=00"
    );

    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_expired_signature_returns_410() {
    let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key_bytes = hex::decode(key_hex).unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

    let (_state, app) = test_setup(AuthMode::Signed, keys, 1000, 100, Vec::new());

    // Expired 60s ago (> 30s tolerance)
    let exp = now_secs() - 60;
    let params = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo.jpg".to_string(),
        ),
        ("exp".to_string(), exp.to_string()),
    ];
    let canonical = build_canonical_string(exp, &params);
    let sig = compute_signature(&key_bytes, &canonical);

    let uri =
        format!("/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&exp={exp}&sig={sig}");

    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::GONE);
}

#[tokio::test]
async fn test_far_future_expiry_rejected_with_401() {
    let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key_bytes = hex::decode(key_hex).unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

    let (_state, app) = test_setup(AuthMode::Signed, keys, 1000, 100, Vec::new());

    // Far future: now + 100,000s (> 86,400s max TTL)
    let exp = now_secs() + 100_000;
    let params = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo.jpg".to_string(),
        ),
        ("exp".to_string(), exp.to_string()),
    ];
    let canonical = build_canonical_string(exp, &params);
    let sig = compute_signature(&key_bytes, &canonical);

    let uri =
        format!("/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&exp={exp}&sig={sig}");

    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_key_rotation_multi_keys() {
    let k1_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let k2_hex = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
    let k1_bytes = hex::decode(k1_hex).unwrap();
    let k2_bytes = hex::decode(k2_hex).unwrap();

    let mut keys = HashMap::new();
    keys.insert("key1".to_string(), k1_bytes.clone());
    keys.insert("key2".to_string(), k2_bytes.clone());

    let (_state, app) = test_setup(AuthMode::Signed, keys, 1000, 100, Vec::new());

    let exp = now_secs() + 3600;

    // Request 1 signed with key1
    let params1 = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo.jpg".to_string(),
        ),
        ("kid".to_string(), "key1".to_string()),
        ("exp".to_string(), exp.to_string()),
    ];
    let canonical1 = build_canonical_string(exp, &params1);
    let sig1 = compute_signature(&k1_bytes, &canonical1);

    let uri1 = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&kid=key1&exp={exp}&sig={sig1}"
    );
    let req1 = Request::builder()
        .method("GET")
        .uri(uri1)
        .body(Body::empty())
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Request 2 signed with key2
    let params2 = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo.jpg".to_string(),
        ),
        ("kid".to_string(), "key2".to_string()),
        ("exp".to_string(), exp.to_string()),
    ];
    let canonical2 = build_canonical_string(exp, &params2);
    let sig2 = compute_signature(&k2_bytes, &canonical2);

    let uri2 = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&kid=key2&exp={exp}&sig={sig2}"
    );
    let req2 = Request::builder()
        .method("GET")
        .uri(uri2)
        .body(Body::empty())
        .unwrap();
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    // Unknown key ID returns 401
    let uri3 = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&kid=unknown_key&exp={exp}&sig={sig1}"
    );
    let req3 = Request::builder()
        .method("GET")
        .uri(uri3)
        .body(Body::empty())
        .unwrap();
    let resp3 = app.oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::UNAUTHORIZED);
}

// -----------------------------------------------------------------------------
// Rate Limiting Tests
// -----------------------------------------------------------------------------

#[tokio::test]
async fn test_rate_limiter_exceeded_returns_429_with_retry_after() {
    let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key_bytes = hex::decode(key_hex).unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

    // Rate limit = 2 per minute
    let (_state, app) = test_setup(AuthMode::Signed, keys, 2, 2, Vec::new());

    let exp = now_secs() + 3600;
    let params = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo.jpg".to_string(),
        ),
        ("exp".to_string(), exp.to_string()),
    ];
    let canonical = build_canonical_string(exp, &params);
    let sig = compute_signature(&key_bytes, &canonical);
    let uri =
        format!("/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&exp={exp}&sig={sig}");

    // Request 1 -> 200
    let req1 = Request::builder().uri(&uri).body(Body::empty()).unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Request 2 -> 200
    let req2 = Request::builder().uri(&uri).body(Body::empty()).unwrap();
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    // Request 3 -> 429
    let req3 = Request::builder().uri(&uri).body(Body::empty()).unwrap();
    let resp3 = app.oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::TOO_MANY_REQUESTS);

    let retry_after = resp3
        .headers()
        .get("retry-after")
        .unwrap()
        .to_str()
        .unwrap();
    let retry_secs: u64 = retry_after.parse().unwrap();
    assert!(retry_secs >= 1);

    // Verify structured JSON error
    let body = resp3.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "RATE_LIMITED");
    assert!(json.get("request_id").is_some());
    assert!(json.get("hint").is_some());
}

#[tokio::test]
async fn test_stricter_miss_limiter_blocks_expensive_work_but_serves_hits() {
    let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key_bytes = hex::decode(key_hex).unwrap();
    let mut keys = HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

    // Global limit = 100, Miss limit = 1
    let (_state, app) = test_setup(AuthMode::Signed, keys, 100, 1, Vec::new());

    let exp = now_secs() + 3600;

    // First request: photo1.jpg (MISS -> allowed, exhausts miss limit of 1)
    let p1 = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo1.jpg".to_string(),
        ),
        ("format".to_string(), "webp".to_string()),
        ("exp".to_string(), exp.to_string()),
    ];
    let sig1 = compute_signature(&key_bytes, &build_canonical_string(exp, &p1));
    let uri1 = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto1.jpg&format=webp&exp={exp}&sig={sig1}"
    );

    let req1 = Request::builder().uri(&uri1).body(Body::empty()).unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(resp1.headers().get("x-atihsrah-cache").unwrap(), "MISS");

    // Second request: photo2.jpg (new image -> would be MISS -> rejected by miss limiter!)
    let p2 = vec![
        (
            "src".to_string(),
            "https://images.example.com/photo2.jpg".to_string(),
        ),
        ("format".to_string(), "webp".to_string()),
        ("exp".to_string(), exp.to_string()),
    ];
    let sig2 = compute_signature(&key_bytes, &build_canonical_string(exp, &p2));
    let uri2 = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto2.jpg&format=webp&exp={exp}&sig={sig2}"
    );

    let req2 = Request::builder().uri(&uri2).body(Body::empty()).unwrap();
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(resp2.headers().contains_key("retry-after"));

    // Third request: photo1.jpg again (HIT from cache -> allowed despite miss limit exhausted!)
    let req3 = Request::builder().uri(&uri1).body(Body::empty()).unwrap();
    let resp3 = app.oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);
    assert_eq!(resp3.headers().get("x-atihsrah-cache").unwrap(), "HIT");
}

#[tokio::test]
async fn test_public_mode_ip_rate_limiting_and_trusted_proxies() {
    let trusted_cidrs: Vec<ipnet::IpNet> = vec!["10.0.0.0/8".parse().unwrap()];
    // Rate limit 2 per minute
    let (_state, app) = test_setup(AuthMode::Public, HashMap::new(), 2, 2, trusted_cidrs);

    let uri = "/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=100";

    // Request from trusted proxy (10.0.0.1) forwarding client 203.0.113.10
    let mut req1 = Request::builder()
        .uri(uri)
        .header("x-forwarded-for", "203.0.113.10")
        .body(Body::empty())
        .unwrap();
    req1.extensions_mut()
        .insert(ConnectInfo("10.0.0.1:54321".parse::<SocketAddr>().unwrap()));

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Second request for client 203.0.113.10
    let mut req2 = Request::builder()
        .uri(uri)
        .header("x-forwarded-for", "203.0.113.10")
        .body(Body::empty())
        .unwrap();
    req2.extensions_mut()
        .insert(ConnectInfo("10.0.0.1:54321".parse::<SocketAddr>().unwrap()));

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    // Third request for client 203.0.113.10 -> 429
    let mut req3 = Request::builder()
        .uri(uri)
        .header("x-forwarded-for", "203.0.113.10")
        .body(Body::empty())
        .unwrap();
    req3.extensions_mut()
        .insert(ConnectInfo("10.0.0.1:54321".parse::<SocketAddr>().unwrap()));

    let resp3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::TOO_MANY_REQUESTS);

    // Different client IP 203.0.113.20 through the same trusted proxy still has quota!
    let mut req4 = Request::builder()
        .uri(uri)
        .header("x-forwarded-for", "203.0.113.20")
        .body(Body::empty())
        .unwrap();
    req4.extensions_mut()
        .insert(ConnectInfo("10.0.0.1:54321".parse::<SocketAddr>().unwrap()));

    let resp4 = app.oneshot(req4).await.unwrap();
    assert_eq!(resp4.status(), StatusCode::OK);
}
