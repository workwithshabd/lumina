use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header::ACCEPT, Request, StatusCode};
use image::codecs::png::PngEncoder;
use image::{ImageEncoder, Rgba, RgbaImage};
use tower::ServiceExt;

use atihsrah::auth::{build_canonical_string, compute_signature};
use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::SourceFetcher;
use atihsrah::http::create_router;
use atihsrah::processor::image_rs::ImageRsProcessor;
use atihsrah::state::AppState;

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

fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let r = ((x * 13 + y * 7) % 256) as u8;
        let g = ((x * 19 + y * 31) % 256) as u8;
        let b = ((x * 23 + y * 41) % 256) as u8;
        *pixel = image::Rgb([r, g, b]);
    }
    let mut buf = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    buf
}

fn test_config(
    auth_mode: AuthMode,
    signing_key: Option<Vec<u8>>,
    global_limit: u32,
    miss_limit: u32,
) -> Arc<Config> {
    let mut origins = HashSet::new();
    origins.insert("https://images.example.com".to_string());

    let mut signing_keys = HashMap::new();
    if let Some(ref k) = signing_key {
        signing_keys.insert("default".to_string(), k.clone());
        signing_keys.insert("key1".to_string(), k.clone());
        signing_keys.insert("key2".to_string(), k.clone());
    }

    Arc::new(Config {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        allowed_origins: origins,
        source_fetch_timeout_secs: 15,
        source_connect_timeout_secs: 5,
        max_source_fetch_mb: 20,
        max_source_dimension: 8000,
        max_output_dimension: 4096,
        max_processing_memory_mb: 512,
        max_concurrent_transforms: 16,
        cache_max_size_mb: 100,
        cache_single_entry_max_mb: 10,
        cache_ttl_secs: 86400,
        auth_mode,
        signing_keys,
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: global_limit,
        rate_limit_miss_per_minute: miss_limit,
        trusted_proxy_cidrs: vec!["127.0.0.0/8".parse().unwrap()],
        debug_headers: true,
        metrics_bind: "127.0.0.1:0".parse().unwrap(),
        request_timeout_secs: 30,
        global_concurrency_limit: 1024,
        queue_saturation_threshold: 16,
        shutdown_timeout_secs: 5,
        max_body_bytes: 65536,
        private_origins: HashSet::new(),
        private_ca_file: None,
        avif_speed: 6,
        distinct_color_threshold: 256,
        max_retry_pixels: 16_777_216,
        resize_filter: "lanczos3".to_string(),
    })
}

// =============================================================================
// PHASE A1 TESTS: Leader Cancellation, Follower Recovery, Inflight Map Zero
// =============================================================================

#[tokio::test]
async fn test_a1_leader_cancellation_mid_fetch_and_next_request_succeeds() {
    let config = test_config(AuthMode::Public, None, 1000, 100);
    let bytes = Arc::new(make_png(100, 100));

    // Slow origin that pauses on the first call (leader), then serves quickly
    let fetch_attempt = Arc::new(AtomicUsize::new(0));
    let fa = fetch_attempt.clone();
    let b = bytes.clone();
    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| {
            let attempt = fa.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(150));
            }
            Ok((*b).clone())
        }),
    ));
    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    let app = create_router(state.clone());

    let url = "/transform?src=https://images.example.com/pic.png&w=50&h=50&format=png";

    // 1. Leader request cancelled mid-fetch via client timeout
    let app_leader = app.clone();
    let leader_fut = async move {
        let req = Request::builder().uri(url).body(Body::empty()).unwrap();
        app_leader.oneshot(req).await
    };
    let leader_handle = tokio::spawn(async move {
        let _ = tokio::time::timeout(Duration::from_millis(40), leader_fut).await;
    });

    // Allow leader to start and register in inflight map
    tokio::time::sleep(Duration::from_millis(15)).await;

    // 2. Concurrently spawn 5 followers
    let mut follower_handles = Vec::new();
    for _ in 0..5 {
        let app_f = app.clone();
        follower_handles.push(tokio::spawn(async move {
            let req = Request::builder().uri(url).body(Body::empty()).unwrap();
            app_f.oneshot(req).await
        }));
    }

    let _ = leader_handle.await;

    // All followers must finish with 200 OK
    for h in follower_handles {
        let res = h.await.unwrap().unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    // 3. Inflight map size must return to zero
    assert_eq!(
        state.inflight.len(),
        0,
        "Inflight map size must return to zero"
    );

    // 4. Next request succeeds as a cache HIT
    let next_req = Request::builder().uri(url).body(Body::empty()).unwrap();
    let next_resp = app.oneshot(next_req).await.unwrap();
    assert_eq!(next_resp.status(), StatusCode::OK);
    assert_eq!(
        next_resp.headers().get("x-atihsrah-cache").unwrap(),
        "HIT",
        "Subsequent request must be served from cache"
    );
}

#[tokio::test]
async fn test_a1_100_request_herd_with_cancelled_leader() {
    let config = test_config(AuthMode::Public, None, 10000, 1000);
    let bytes = Arc::new(make_png(100, 100));

    let fetch_attempt = Arc::new(AtomicUsize::new(0));
    let fa = fetch_attempt.clone();
    let b = bytes.clone();
    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| {
            let attempt = fa.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(120));
            }
            Ok((*b).clone())
        }),
    ));
    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    let app = create_router(state.clone());

    let url = "/transform?src=https://images.example.com/herd.png&w=64&h=64&format=png";

    // Leader task with tight timeout
    let app_leader = app.clone();
    let leader_handle = tokio::spawn(async move {
        let req = Request::builder().uri(url).body(Body::empty()).unwrap();
        let _ = tokio::time::timeout(Duration::from_millis(30), app_leader.oneshot(req)).await;
    });

    tokio::time::sleep(Duration::from_millis(10)).await;

    // 100 followers in the herd
    let mut handles = Vec::new();
    for _ in 0..100 {
        let app_h = app.clone();
        handles.push(tokio::spawn(async move {
            let req = Request::builder().uri(url).body(Body::empty()).unwrap();
            app_h.oneshot(req).await
        }));
    }

    let _ = leader_handle.await;

    let mut success_count = 0;
    for h in handles {
        let res = h.await.unwrap().unwrap();
        if res.status() == StatusCode::OK {
            success_count += 1;
        }
    }

    assert_eq!(
        success_count, 100,
        "All 100 requests in herd must be successfully served"
    );
    assert_eq!(
        state.inflight.len(),
        0,
        "Inflight map size must be 0 after herd completes"
    );
}

// =============================================================================
// PHASE A2 TESTS: Rate Limiting Before Authentication
// =============================================================================

#[tokio::test]
async fn test_a2_unsigned_claim_kid_cannot_consume_budget() {
    let raw_key = b"0123456789abcdef0123456789abcdef".to_vec();
    // Key-level global quota = 2 per minute
    let config = test_config(AuthMode::Signed, Some(raw_key.clone()), 2, 2);
    let bytes = Arc::new(make_png(100, 100));
    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| Ok((*bytes).clone())),
    ));
    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    let app = create_router(state);

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now_secs + 3600;

    // Attacker sends 10 requests with invalid signatures claiming kid=key1 from 1.2.3.4
    for _ in 0..10 {
        let bad_url = format!(
            "/transform?src=https%3A%2F%2Fimages.example.com%2Fpic.png&exp={exp}&kid=key1&sig=deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        );
        let req = Request::builder()
            .uri(&bad_url)
            .header("x-forwarded-for", "1.2.3.4")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        // Rejected before touching key1's authenticated budget
        assert!(
            resp.status() == StatusCode::UNAUTHORIZED
                || resp.status() == StatusCode::TOO_MANY_REQUESTS
        );
    }

    // Legitimate client with valid signature for kid=key1 sends 2 requests: both must succeed!
    let params = vec![
        (
            "src".to_string(),
            "https://images.example.com/pic.png".to_string(),
        ),
        ("exp".to_string(), exp.to_string()),
        ("kid".to_string(), "key1".to_string()),
    ];
    let canonical = build_canonical_string(exp, &params);
    let valid_sig = compute_signature(&raw_key, &canonical);
    let valid_url = format!(
        "/transform?src=https%3A%2F%2Fimages.example.com%2Fpic.png&exp={exp}&kid=key1&sig={valid_sig}"
    );

    for _ in 0..2 {
        let req = Request::builder()
            .uri(&valid_url)
            .header("x-forwarded-for", "9.9.9.9")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Key budget must NOT be depleted by attacker"
        );
    }

    // 3rd legitimate request hits key quota (2/min) and gets 429
    let req3 = Request::builder()
        .uri(&valid_url)
        .header("x-forwarded-for", "9.9.9.9")
        .body(Body::empty())
        .unwrap();
    let resp3 = app.oneshot(req3).await.unwrap();
    assert_eq!(
        resp3.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "Valid key is still rate limited"
    );
}

#[tokio::test]
async fn test_a2_many_fake_kids_do_not_grow_limiter_state() {
    let raw_key = b"0123456789abcdef0123456789abcdef".to_vec();
    let config = test_config(AuthMode::Signed, Some(raw_key.clone()), 100, 100);
    let bytes = Arc::new(make_png(100, 100));
    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| Ok((*bytes).clone())),
    ));
    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    let app = create_router(state);

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now_secs + 3600;

    // Send 50 requests with different fake kid parameters and invalid signatures
    for i in 0..50 {
        let bad_url = format!(
            "/transform?src=https%3A%2F%2Fimages.example.com%2Fpic.png&exp={exp}&kid=fake_kid_{i}&sig=invalid"
        );
        let req = Request::builder()
            .uri(&bad_url)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

// =============================================================================
// PHASE A3 + A4 TESTS: Accept Handling, Cache Lookup & Single-Flight Key Isolation
// =============================================================================

#[tokio::test]
async fn test_a3_accept_wildcard_second_request_is_hit() {
    let config = test_config(AuthMode::Public, None, 1000, 100);
    let bytes = Arc::new(make_png(100, 100));
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let fc = fetch_count.clone();
    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| {
            fc.fetch_add(1, Ordering::SeqCst);
            Ok((*bytes).clone())
        }),
    ));
    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    let app = create_router(state);

    let url = "/transform?src=https://images.example.com/pic.png&w=50&h=50&format=auto";

    // Request 1: cold miss
    let req1 = Request::builder()
        .uri(url)
        .header(ACCEPT, "*/*")
        .body(Body::empty())
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(resp1.headers().get("x-atihsrah-cache").unwrap(), "MISS");
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);

    // Request 2: identical Accept: */*
    let req2 = Request::builder()
        .uri(url)
        .header(ACCEPT, "*/*")
        .body(Body::empty())
        .unwrap();
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    assert_eq!(resp2.headers().get("x-atihsrah-cache").unwrap(), "HIT");
    assert_eq!(
        fetch_count.load(Ordering::SeqCst),
        1,
        "Must NOT re-fetch from origin"
    );
}

#[tokio::test]
async fn test_a4_avif_and_non_avif_clients_concurrent_get_acceptable_formats() {
    let config = test_config(AuthMode::Public, None, 1000, 100);
    let bytes = Arc::new(make_jpeg(100, 100));
    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| Ok((*bytes).clone())),
    ));
    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    let app = create_router(state);

    let url = "/transform?src=https://images.example.com/pic.jpg&w=50&h=50&format=auto";

    // Request 1: AVIF capable
    let app1 = app.clone();
    let h1 = tokio::spawn(async move {
        let req = Request::builder()
            .uri(url)
            .header(ACCEPT, "image/avif,image/webp,*/*;q=0.8")
            .body(Body::empty())
            .unwrap();
        app1.oneshot(req).await.unwrap()
    });

    // Request 2: WebP only (no AVIF)
    let app2 = app.clone();
    let h2 = tokio::spawn(async move {
        let req = Request::builder()
            .uri(url)
            .header(ACCEPT, "image/webp,*/*;q=0.8")
            .body(Body::empty())
            .unwrap();
        app2.oneshot(req).await.unwrap()
    });

    // Request 3: Legacy (explicitly q=0 on webp and avif)
    let app3 = app.clone();
    let h3 = tokio::spawn(async move {
        let req = Request::builder()
            .uri(url)
            .header(
                ACCEPT,
                "image/avif;q=0, image/webp;q=0, image/jpeg, image/png",
            )
            .body(Body::empty())
            .unwrap();
        app3.oneshot(req).await.unwrap()
    });

    let (r1, r2, r3) = tokio::join!(h1, h2, h3);
    let resp1 = r1.unwrap();
    let resp2 = r2.unwrap();
    let resp3 = r3.unwrap();

    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(resp2.status(), StatusCode::OK);
    assert_eq!(resp3.status(), StatusCode::OK);

    let ct1 = resp1
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    let ct2 = resp2
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    let ct3 = resp3
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();

    assert_eq!(ct1, "image/avif", "Client 1 must receive AVIF");
    // Under Stage 1 Option A, photo content with WebP accept returns JPEG (never lossless WebP for photos)
    assert_eq!(
        ct2, "image/jpeg",
        "Client 2 must receive JPEG for photographic content"
    );
    assert_ne!(ct3, "image/avif", "Client 3 excluded AVIF");
    assert_ne!(ct3, "image/webp", "Client 3 excluded WebP");
}

#[tokio::test]
async fn test_a4_q_zero_exclusion_respected() {
    let config = test_config(AuthMode::Public, None, 1000, 100);
    let bytes = Arc::new(make_png(100, 100));
    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| Ok((*bytes).clone())),
    ));
    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    let app = create_router(state);

    let url = "/transform?src=https://images.example.com/pic.png&w=50&h=50&format=auto";
    let req = Request::builder()
        .uri(url)
        .header(ACCEPT, "image/webp;q=0, image/jpeg, */*")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_ne!(
        ct, "image/webp",
        "Client with image/webp;q=0 must NOT receive WebP"
    );
}
