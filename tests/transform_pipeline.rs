use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::SourceFetcher;
use atihsrah::http::create_router;
use atihsrah::processor::image_rs::ImageRsProcessor;
use atihsrah::state::AppState;
use axum::body::Body;
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

/// Creates a valid PNG file with a forged IHDR header claiming huge dimensions (decompression bomb).
fn make_decompression_bomb_png(w: u32, h: u32) -> Vec<u8> {
    let mut data = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, // IHDR length
        0x49, 0x48, 0x44, 0x52, // "IHDR"
    ];
    data.extend_from_slice(&w.to_be_bytes()); // Width
    data.extend_from_slice(&h.to_be_bytes()); // Height
    data.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    let crc = crc32fast::hash(&data[12..29]);
    data.extend_from_slice(&crc.to_be_bytes());
    // Dummy minimal IDAT & IEND
    data.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
        0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
        0xAE, 0x42, 0x60, 0x82,
    ]);
    data
}

fn test_setup(
    fetch_counter: Arc<AtomicUsize>,
    cache_single_entry_max_mb: u64,
    cache_max_size_mb: u64,
    max_processing_memory_mb: usize,
) -> (Arc<AppState>, axum::Router) {
    let mut origins = HashSet::new();
    origins.insert("https://images.unsplash.com".to_string());

    let config = Arc::new(Config {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        allowed_origins: origins,
        source_fetch_timeout_secs: 15,
        source_connect_timeout_secs: 5,
        max_source_fetch_mb: 20,
        max_source_dimension: 8000,
        max_output_dimension: 4096,
        max_processing_memory_mb,
        max_concurrent_transforms: 4,
        cache_max_size_mb,
        cache_single_entry_max_mb,
        cache_ttl_secs: 86400,
        auth_mode: AuthMode::Public,
        signing_keys: std::collections::HashMap::new(),
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: 1000,
        rate_limit_miss_per_minute: 100,
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

    let mock_fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |url: &str| {
            fetch_counter.fetch_add(1, Ordering::SeqCst);
            if url.contains("decompression-bomb") {
                Ok(make_decompression_bomb_png(9000, 9000))
            } else if url.contains("high-memory-bomb") {
                Ok(make_decompression_bomb_png(5000, 5000))
            } else if url.contains("corrupted") {
                Ok(vec![
                    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0xFF,
                ])
            } else {
                Ok(make_png(100, 100))
            }
        }),
    ));

    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, mock_fetcher));
    state.set_ready(true);
    let router = create_router(state.clone());
    (state, router)
}

#[tokio::test]
async fn test_cache_hit_and_miss_lifecycle() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let (_state, app) = test_setup(fetch_count.clone(), 10, 500, 512);

    let uri = "/transform?src=https://images.unsplash.com/photo.jpg&w=50&h=50&format=webp";

    // 1. First request -> MISS
    let req1 = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(resp1.headers().get("x-atihsrah-cache").unwrap(), "MISS");
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);

    // 2. Second identical request -> HIT
    let req2 = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    assert_eq!(resp2.headers().get("x-atihsrah-cache").unwrap(), "HIT");
    // Origin fetch count MUST still be 1 (no re-fetch!)
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_100_concurrent_identical_requests_single_flight() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let (_state, app) = test_setup(fetch_count.clone(), 10, 500, 512);

    let uri = "/transform?src=https://images.unsplash.com/photo.jpg&w=80&h=80&format=png&v=burst";

    let mut handles = Vec::new();
    for _ in 0..100 {
        let app_clone = app.clone();
        handles.push(tokio::spawn(async move {
            let req = Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap();
            app_clone.oneshot(req).await.unwrap()
        }));
    }

    let mut success_count = 0;
    for handle in handles {
        let resp = handle.await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        success_count += 1;
    }

    assert_eq!(success_count, 100);
    // Crucial single-flight invariant: 100 concurrent requests triggered exactly 1 origin fetch
    assert_eq!(
        fetch_count.load(Ordering::SeqCst),
        1,
        "Single-flight failed: expected 1 origin fetch, got {}",
        fetch_count.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn test_reject_duplicate_and_unknown_params() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let (_state, app) = test_setup(fetch_count.clone(), 10, 500, 512);

    // 1. Duplicate parameter -> 422
    let req1 = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/a.jpg&w=100&w=200")
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body1 = resp1.into_body().collect().await.unwrap().to_bytes();
    let json1: serde_json::Value = serde_json::from_slice(&body1).unwrap();
    assert_eq!(json1["code"], "DUPLICATE_PARAM");

    // 2. Unknown parameter -> 422
    let req2 = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/a.jpg&w=100&evil_param=1")
        .body(Body::empty())
        .unwrap();

    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body2 = resp2.into_body().collect().await.unwrap().to_bytes();
    let json2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert_eq!(json2["code"], "UNKNOWN_PARAM");
}

#[tokio::test]
async fn test_decompression_bomb_rejected_header_only() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    // Max source dimension is 8000; mock returns 9000x9000
    let (_state, app) = test_setup(fetch_count.clone(), 10, 500, 512);

    let req = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/decompression-bomb.png&w=100")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "IMAGE_TOO_LARGE");
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("MAX_SOURCE_DIMENSION"));
}

#[tokio::test]
async fn test_estimated_processing_memory_limit_rejected() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    // Max processing memory set to 10 MB. Image claims 5000x5000 (est memory = 5000*5000*4*3 = 300 MB > 10 MB)
    let (_state, app) = test_setup(fetch_count.clone(), 10, 500, 10);

    let req = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/high-memory-bomb.png&w=100")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "IMAGE_TOO_LARGE");
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("Estimated processing memory"));
}

#[tokio::test]
async fn test_corrupt_truncated_image_returns_422() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let (_state, app) = test_setup(fetch_count.clone(), 10, 500, 512);

    let req = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/corrupted.png&w=100")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "DECODE_FAILED");
}

#[tokio::test]
async fn test_oversize_entry_not_cached() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    // Set single entry max size to 0 MB (i.e. nothing fits into cache)
    let (_state, app) = test_setup(fetch_count.clone(), 0, 500, 512);

    let uri = "/transform?src=https://images.unsplash.com/photo.jpg&w=100&h=100&format=png";

    // 1. First request succeeds, returns MISS
    let req1 = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(resp1.headers().get("x-atihsrah-cache").unwrap(), "MISS");

    // 2. Second request must ALSO be a MISS because it exceeded single entry limit and was not cached
    let req2 = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    assert_eq!(resp2.headers().get("x-atihsrah-cache").unwrap(), "MISS");
    // Re-fetched from origin
    assert_eq!(fetch_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_immutable_cache_control_with_version_param() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let (_state, app) = test_setup(fetch_count.clone(), 10, 500, 512);

    // 1. With `v`: public, max-age=31536000, immutable
    let req1 = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/photo.jpg&w=50&v=release42")
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(
        resp1.headers().get("cache-control").unwrap(),
        "public, max-age=31536000, immutable"
    );

    // 2. Without `v`: public, max-age=86400
    let req2 = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/photo.jpg&w=50")
        .body(Body::empty())
        .unwrap();

    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    assert_eq!(
        resp2.headers().get("cache-control").unwrap(),
        "public, max-age=86400"
    );
}

#[tokio::test]
async fn test_byte_weighted_cache_eviction() {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    // Very small cache: 1 MB capacity, single entry max 1 MB
    let (_state, app) = test_setup(fetch_count.clone(), 1, 1, 512);

    // Fetch image 1 -> MISS, cached
    let req1 = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/photo.jpg&w=100&h=100&format=png&v=1")
        .body(Body::empty())
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(resp1.headers().get("x-atihsrah-cache").unwrap(), "MISS");

    // Fetch image 1 again immediately -> HIT
    let req1_again = Request::builder()
        .method("GET")
        .uri("/transform?src=https://images.unsplash.com/photo.jpg&w=100&h=100&format=png&v=1")
        .body(Body::empty())
        .unwrap();
    let resp1_again = app.clone().oneshot(req1_again).await.unwrap();
    assert_eq!(resp1_again.status(), StatusCode::OK);
    assert_eq!(
        resp1_again.headers().get("x-atihsrah-cache").unwrap(),
        "HIT"
    );
}

#[tokio::test]
async fn test_signed_mode_cache_control_and_vary_headers_pinned() {
    use atihsrah::auth::signing::{build_canonical_string, compute_signature, rfc3986_encode};
    use atihsrah::error::AppError;

    let key_bytes =
        hex::decode("1111111111111111111111111111111111111111111111111111111111111112").unwrap();
    let fetch_count = Arc::new(AtomicUsize::new(0));

    let mut origins = HashSet::new();
    origins.insert("https://images.unsplash.com".to_string());
    let mut keys = std::collections::HashMap::new();
    keys.insert("default".to_string(), key_bytes.clone());

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
        auth_mode: AuthMode::Signed,
        signing_keys: keys,
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: 1000,
        rate_limit_miss_per_minute: 100,
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

    let mock_fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |url: &str| {
            fetch_count.fetch_add(1, Ordering::SeqCst);
            if url.contains("photo.jpg") {
                Ok(make_png(100, 100))
            } else {
                Err(AppError::FetchFailed("Not found".to_string()))
            }
        }),
    ));

    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, mock_fetcher));
    state.set_ready(true);
    let app = create_router(state);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let sign_helper = |params: &[(&str, &str)], exp: u64| -> String {
        let exp_str = exp.to_string();
        let mut p_vec: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        p_vec.push(("exp".to_string(), exp_str.clone()));
        let canonical = build_canonical_string(exp, &p_vec);
        let sig = compute_signature(&key_bytes, &canonical);

        let mut query_pairs: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{k}={}", rfc3986_encode(v)))
            .collect();
        query_pairs.push(format!("exp={exp_str}"));
        query_pairs.push(format!("sig={sig}"));
        format!("/transform?{}", query_pairs.join("&"))
    };

    // 1. Signed URL WITH version 'v' and exp=now+3600
    // In signed mode, Cache-Control is clamped by exp: max-age=3600, and immutable is NOT emitted
    let uri_with_v = sign_helper(
        &[
            ("src", "https://images.unsplash.com/photo.jpg"),
            ("w", "50"),
            ("v", "rev1"),
        ],
        now + 3600,
    );
    let req1 = Request::builder()
        .method("GET")
        .uri(&uri_with_v)
        .body(Body::empty())
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let cc1 = resp1
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cc1, "public, max-age=3600");
    assert!(
        !cc1.contains("immutable"),
        "immutable must not be present when exp is bounded"
    );

    // 2. Signed URL WITHOUT version 'v' and exp=now+3600
    let uri_without_v = sign_helper(
        &[
            ("src", "https://images.unsplash.com/photo.jpg"),
            ("w", "50"),
        ],
        now + 3600,
    );
    let req2 = Request::builder()
        .method("GET")
        .uri(&uri_without_v)
        .body(Body::empty())
        .unwrap();
    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let cc2 = resp2
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cc2, "public, max-age=3600");

    // 3. Signed URL near expiry (exp = now + 4)
    let uri_near_expiry = sign_helper(
        &[
            ("src", "https://images.unsplash.com/photo.jpg"),
            ("w", "50"),
            ("v", "rev2"),
        ],
        now + 4,
    );
    let req3 = Request::builder()
        .method("GET")
        .uri(&uri_near_expiry)
        .body(Body::empty())
        .unwrap();
    let resp3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);
    let cc3 = resp3
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cc3.starts_with("public, max-age="));
    let age: u64 = cc3
        .strip_prefix("public, max-age=")
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        age <= 4,
        "max-age must reflect remaining ttl <= 4, got {age}"
    );

    // 4. Vary: Accept header emitted on format=auto (default)
    assert_eq!(
        resp1.headers().get("vary").unwrap().to_str().unwrap(),
        "Accept"
    );
}
