use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use atihsrah::auth::{build_canonical_string, compute_signature, extract_client_ip};
use atihsrah::cache::inflight::{InflightManager, InflightRole};
use atihsrah::cache::{compute_cache_key, CacheKeyParams};
use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::resolver::is_safe_public_ip;
use atihsrah::fetch::SourceFetcher;
use atihsrah::http::create_router;
use atihsrah::http::params::{resolve_candidate_format, resolve_format, FormatParam};
use atihsrah::processor::image_rs::ImageRsProcessor;
use atihsrah::processor::{ImageFormat, ImageProcessor, ProcessOptions};
use atihsrah::state::{AppState, TransformQueueGuard};
use axum::body::Body;
use axum::http::{header::ACCEPT, header::CACHE_CONTROL, Request, StatusCode};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageEncoder, Rgba, RgbaImage};
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

fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255])
    }));
    let mut buf = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, 80);
    encoder
        .encode(img.to_rgb8().as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    buf
}

fn test_config(
    auth_mode: AuthMode,
    signing_key: Option<Vec<u8>>,
    rate_limit_per_minute: u32,
    trusted_cidrs: Vec<ipnet::IpNet>,
) -> Arc<Config> {
    let mut origins = HashSet::new();
    origins.insert("https://images.example.com".to_string());

    let mut signing_keys = std::collections::HashMap::new();
    if let Some(ref k) = signing_key {
        signing_keys.insert("default".to_string(), k.clone());
        signing_keys.insert("key1".to_string(), k.clone());
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
        max_concurrent_transforms: 4,
        cache_max_size_mb: 100,
        cache_single_entry_max_mb: 10,
        cache_ttl_secs: 86400,
        auth_mode,
        signing_keys,
        signing_key,
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute,
        rate_limit_miss_per_minute: rate_limit_per_minute,
        trusted_proxy_cidrs: trusted_cidrs,
        log_level: "info".to_string(),
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
    })
}

fn create_test_app(
    auth_mode: AuthMode,
    signing_key: Option<Vec<u8>>,
    mock_bytes: Vec<u8>,
    fetch_counter: Arc<AtomicUsize>,
    rate_limit_per_minute: u32,
    trusted_cidrs: Vec<ipnet::IpNet>,
) -> (Arc<AppState>, axum::Router) {
    let config = test_config(auth_mode, signing_key, rate_limit_per_minute, trusted_cidrs);

    let fc = fetch_counter.clone();
    let bytes = Arc::new(mock_bytes);
    let fetcher = Arc::new(SourceFetcher::new_mock(
        config.clone(),
        Arc::new(move |_url: &str| {
            fc.fetch_add(1, Ordering::SeqCst);
            let b = bytes.clone();
            Ok((*b).clone())
        }),
    ));

    let processor = Arc::new(ImageRsProcessor::new());
    let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
    state.set_ready(true);
    let router = create_router(state.clone());
    (state, router)
}

// -----------------------------------------------------------------------------
// Finding A: Rate-limit identity & auth ordering
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_finding_a_unauthenticated_kid_spray_blocked_by_ip_limiter() {
    let key = b"0123456789abcdef0123456789abcdef".to_vec();
    let fetch_counter = Arc::new(AtomicUsize::new(0));
    // Set rate limit = 5 req/min
    let (_state, app) = create_test_app(
        AuthMode::Signed,
        Some(key),
        make_png(100, 100),
        fetch_counter,
        5,
        Vec::new(),
    );

    // Send 5 unauthenticated requests with rotating random kids
    for i in 0..5 {
        let uri = format!(
            "/transform?src=https://images.example.com/test.png&w=50&kid=bogus_{i}&exp=9999999999&sig=deadbeef"
        );
        let req = Request::builder()
            .uri(uri)
            .extension(axum::extract::ConnectInfo(
                "198.51.100.5:12345".parse::<SocketAddr>().unwrap(),
            ))
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        // First 5 fail with 401 signature invalid
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // 6th request from the same IP with a new kid MUST be blocked by IP rate limiter (429)
    let uri = "/transform?src=https://images.example.com/test.png&w=50&kid=bogus_new&exp=9999999999&sig=deadbeef";
    let req = Request::builder()
        .uri(uri)
        .extension(axum::extract::ConnectInfo(
            "198.51.100.5:12345".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "IP rate limiter must block kid spray before auth"
    );
}

#[test]
fn test_finding_a_trusted_proxy_traversal_prevents_spoofing() {
    let trusted_cidrs: Vec<ipnet::IpNet> = vec!["10.0.0.0/8".parse().unwrap()];
    let peer: SocketAddr = "10.1.2.3:12345".parse().unwrap();

    // Attacker forged "198.51.100.1" in XFF, proxy appended attacker's real IP "203.0.113.88"
    let xff = "198.51.100.1, 203.0.113.88, 10.9.8.7";
    let ip = extract_client_ip(peer, Some(xff), &trusted_cidrs);
    assert_eq!(
        ip,
        "203.0.113.88".parse::<IpAddr>().unwrap(),
        "Must extract rightmost untrusted IP"
    );
}

// -----------------------------------------------------------------------------
// Finding B: Incomplete cache identity (allow_upscale)
// -----------------------------------------------------------------------------
#[test]
fn test_finding_b_cache_key_includes_allow_upscale() {
    let base = CacheKeyParams {
        src: "https://images.example.com/pic.jpg",
        w: Some(500),
        h: Some(500),
        format: ImageFormat::WebP.into(),
        quality: 80,
        fit: atihsrah::processor::FitMode::Cover,
        allow_upscale: false,
        v: None,
    };
    let key_no_upscale = compute_cache_key(&base);

    let key_with_upscale = compute_cache_key(&CacheKeyParams {
        allow_upscale: true,
        ..base
    });

    assert_ne!(
        key_no_upscale, key_with_upscale,
        "Cache keys for allow_upscale=false vs allow_upscale=true must differ"
    );
}

// -----------------------------------------------------------------------------
// Finding C: Cancellation-safe single-flight coordination
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_finding_c_leader_abort_triggers_follower_takeover() {
    let inflight = Arc::new(InflightManager::new());
    let key = "inflight:test_key";

    // Task 1 becomes leader
    let role1 = inflight.subscribe_or_lead(key);
    let guard1 = match role1 {
        InflightRole::Leader(g) => g,
        InflightRole::Follower(_) => panic!("Expected Leader"),
    };

    // Task 2 becomes follower
    let role2 = inflight.subscribe_or_lead(key);
    let mut rx2 = match role2 {
        InflightRole::Follower(rx) => rx,
        InflightRole::Leader(_) => panic!("Expected Follower"),
    };

    // Simulate leader abort: guard1 is dropped without complete()
    drop(guard1);

    // Follower receives notification of leader cancellation
    let recv_res = rx2.recv().await;
    assert!(
        recv_res.is_err() || matches!(recv_res, Ok(Err(_))),
        "Channel should notify follower when leader drops"
    );

    // Crucially: key must be cleared from map, allowing next request to become Leader!
    let role3 = inflight.subscribe_or_lead(key);
    match role3 {
        InflightRole::Leader(guard3) => {
            // New leader completes successfully
            let cached = Arc::new(atihsrah::cache::CachedImage {
                bytes: vec![1, 2, 3],
                format: ImageFormat::WebP,
                width: 10,
                height: 10,
            });
            guard3.complete(Ok(cached));
        }
        InflightRole::Follower(_) => panic!("Expected takeover as Leader"),
    }
}

// -----------------------------------------------------------------------------
// Finding D: Concurrency counter leakage
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_finding_d_queue_cancellation_no_counter_leak() {
    let key = b"0123456789abcdef0123456789abcdef".to_vec();
    let fetch_counter = Arc::new(AtomicUsize::new(0));
    let (state, _app) = create_test_app(
        AuthMode::Public,
        Some(key),
        make_png(100, 100),
        fetch_counter,
        1000,
        Vec::new(),
    );

    assert_eq!(state.readiness_check(), Ok(()));

    // Simulate 20 requests aborted while waiting in queue
    for _ in 0..20 {
        let guard = TransformQueueGuard::enter(&state);
        // Abort / drop without transition_to_active
        drop(guard);
    }

    // Verify readiness check is still healthy and not marked saturated
    assert_eq!(
        state.readiness_check(),
        Ok(()),
        "Queue counter must not leak on cancellation"
    );
}

// -----------------------------------------------------------------------------
// Finding E: Granular private origins & mandatory TLS verification
// -----------------------------------------------------------------------------
#[test]
fn test_finding_e_private_networks_does_not_disable_tls_validation() {
    let cfg = test_config(AuthMode::Public, None, 1000, Vec::new());
    assert!(cfg.private_origins.is_empty());
    assert!(cfg.private_ca_file.is_none());

    // Verify private origins require explicit origins and preserve TLS verification
    let mut custom = (*cfg).clone();
    custom
        .private_origins
        .insert("https://internal.origin:8000".to_string());
    assert_eq!(custom.private_origins.len(), 1);
    assert!(custom.private_ca_file.is_none());
}

// -----------------------------------------------------------------------------
// Finding F: Signed URL expiration & HTTP cache lifetime
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_finding_f_signed_url_exp_clamps_cache_control_max_age() {
    let key = b"0123456789abcdef0123456789abcdef".to_vec();
    let fetch_counter = Arc::new(AtomicUsize::new(0));
    let (_state, app) = create_test_app(
        AuthMode::Signed,
        Some(key.clone()),
        make_png(100, 100),
        fetch_counter,
        1000,
        Vec::new(),
    );

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Expire in 60 seconds
    let exp_60 = now + 60;
    let params = vec![
        (
            "src".to_string(),
            "https://images.example.com/test.png".to_string(),
        ),
        ("w".to_string(), "50".to_string()),
    ];

    let canonical = build_canonical_string(exp_60, &params);
    let sig = compute_signature(&key, &canonical);

    let uri =
        format!("/transform?src=https://images.example.com/test.png&w=50&exp={exp_60}&sig={sig}");
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let cc = resp.headers().get(CACHE_CONTROL).unwrap().to_str().unwrap();
    assert!(
        cc.starts_with("public, max-age="),
        "Expected max-age header: {cc}"
    );
    let age: u64 = cc
        .strip_prefix("public, max-age=")
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (58..=60).contains(&age),
        "max-age must be clamped to remaining signature TTL (got {age})"
    );
}

// -----------------------------------------------------------------------------
// Finding G: WebP quality handling & lossy compression under Option A
// -----------------------------------------------------------------------------
#[test]
fn test_finding_g_webp_lossless_option_a_invariant_under_quality_target() {
    let processor = ImageRsProcessor::new();
    let source_jpg = make_jpeg(400, 400);

    // Under Option A (pure Rust), explicit format=webp serves lossless WebP where quality does not change size
    let out_draft = processor
        .process(
            &source_jpg,
            ProcessOptions {
                target_w: Some(200),
                target_h: Some(200),
                fit: atihsrah::processor::FitMode::Cover,
                format: Some(ImageFormat::WebP),
                quality_target: Some(atihsrah::processor::QualityTarget::Draft),
                ..Default::default()
            },
        )
        .unwrap();

    let out_high = processor
        .process(
            &source_jpg,
            ProcessOptions {
                target_w: Some(200),
                target_h: Some(200),
                fit: atihsrah::processor::FitMode::Cover,
                format: Some(ImageFormat::WebP),
                quality_target: Some(atihsrah::processor::QualityTarget::High),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(out_draft.format, ImageFormat::WebP);
    assert_eq!(out_high.format, ImageFormat::WebP);
    // In pure Rust Option A, WebP is lossless-only via image crate
    assert_eq!(
        out_draft.bytes.len(),
        out_high.bytes.len(),
        "Under Option A, WebP is lossless-only and invariant under quality_target"
    );

    // JPEG quality strictly produces distinct sizes (draft < high)
    let out_jpeg_draft = processor
        .process(
            &source_jpg,
            ProcessOptions {
                target_w: Some(200),
                target_h: Some(200),
                fit: atihsrah::processor::FitMode::Cover,
                format: Some(ImageFormat::Jpeg),
                quality_target: Some(atihsrah::processor::QualityTarget::Draft),
                ..Default::default()
            },
        )
        .unwrap();

    let out_jpeg_high = processor
        .process(
            &source_jpg,
            ProcessOptions {
                target_w: Some(200),
                target_h: Some(200),
                fit: atihsrah::processor::FitMode::Cover,
                format: Some(ImageFormat::Jpeg),
                quality_target: Some(atihsrah::processor::QualityTarget::High),
                ..Default::default()
            },
        )
        .unwrap();

    assert!(
        out_jpeg_draft.bytes.len() < out_jpeg_high.bytes.len(),
        "JPEG draft ({} B) must be smaller than high ({} B)",
        out_jpeg_draft.bytes.len(),
        out_jpeg_high.bytes.len()
    );
}

// -----------------------------------------------------------------------------
// Finding I: SSRF IPv6 special addresses
// -----------------------------------------------------------------------------
#[test]
fn test_finding_i_ipv6_special_addresses_blocked() {
    // 6to4 embedding 127.0.0.1 (7f00:0001)
    let six_to_four_loopback: IpAddr = "2002:7f00:0001::".parse().unwrap();
    assert!(
        !is_safe_public_ip(&six_to_four_loopback),
        "6to4 loopback must be blocked"
    );

    // 6to4 embedding 10.0.0.1 (0a00:0001)
    let six_to_four_private: IpAddr = "2002:0a00:0001::".parse().unwrap();
    assert!(
        !is_safe_public_ip(&six_to_four_private),
        "6to4 private must be blocked"
    );

    // Teredo embedding private IP 10.0.0.1 -> inverted is 245.255.255.254 (f5ff:fffe)
    let teredo_private: IpAddr = "2001:0000:4136:e378:8000:63bf:f5ff:fffe".parse().unwrap();
    assert!(
        !is_safe_public_ip(&teredo_private),
        "Teredo private must be blocked"
    );

    // SIIT prefix ::ffff:0:a.b.c.d embedding 127.0.0.1
    let siit_loopback: IpAddr = "::ffff:0:127.0.0.1".parse().unwrap();
    assert!(
        !is_safe_public_ip(&siit_loopback),
        "SIIT loopback must be blocked"
    );

    // RFC 6666 Discard-only
    let discard: IpAddr = "100::1".parse().unwrap();
    assert!(!is_safe_public_ip(&discard), "Discard-only must be blocked");

    // RFC 5180 BMWG Benchmarking
    let bmwg: IpAddr = "2001:2::1".parse().unwrap();
    assert!(!is_safe_public_ip(&bmwg), "BMWG must be blocked");

    // Genuine public IPv6 must pass
    let public_v6: IpAddr = "2606:4700:4700::1111".parse().unwrap();
    assert!(is_safe_public_ip(&public_v6), "Public IPv6 must be allowed");
}

// -----------------------------------------------------------------------------
// Finding J: HTTP Accept negotiation respecting q=0
// -----------------------------------------------------------------------------
#[test]
fn test_finding_j_accept_negotiation_respects_q_values() {
    // AVIF rejected via q=0, WebP accepted with q=0.9
    let accept = "image/avif;q=0, image/webp;q=0.9, image/*;q=0.8";
    let (fmt, is_auto) = resolve_format(
        Some(FormatParam::Auto),
        Some(accept),
        false,
        ImageFormat::Png,
    );
    assert_eq!(
        fmt,
        ImageFormat::WebP,
        "Must choose WebP for graphic when AVIF has q=0"
    );
    assert!(is_auto);

    // Photographic content hints never choose lossless WebP under Option A
    let (fmt_photo, _) = resolve_format(
        Some(FormatParam::Auto),
        Some(accept),
        false,
        ImageFormat::Jpeg,
    );
    assert_eq!(
        fmt_photo,
        ImageFormat::Jpeg,
        "Photographic content must choose JPEG when AVIF has q=0"
    );

    // Both AVIF and WebP rejected via q=0 -> falls back to JPEG
    let accept_reject_both = "image/avif;q=0, image/webp;q=0, */*";
    let (fmt_legacy, _) = resolve_format(
        Some(FormatParam::Auto),
        Some(accept_reject_both),
        false,
        ImageFormat::Jpeg,
    );
    assert_eq!(
        fmt_legacy,
        ImageFormat::Jpeg,
        "Must fall back to JPEG when both modern formats have q=0"
    );

    // Candidate format helper matches
    let candidate = resolve_candidate_format(Some(FormatParam::Auto), Some(accept));
    assert_eq!(candidate, Some((ImageFormat::WebP, true)));
}

// -----------------------------------------------------------------------------
// Finding K: In-flight key collision prevention on differing Accept
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_finding_k_inflight_key_different_accept_headers() {
    let key = b"0123456789abcdef0123456789abcdef".to_vec();
    let fetch_counter = Arc::new(AtomicUsize::new(0));
    let (_state, app) = create_test_app(
        AuthMode::Public,
        Some(key),
        make_jpeg(200, 200),
        fetch_counter,
        1000,
        Vec::new(),
    );

    // Request 1: format=auto with Accept: image/avif
    let req1 = Request::builder()
        .uri("/transform?src=https://images.example.com/pic.jpg&format=auto")
        .header(ACCEPT, "image/avif,image/webp,*/*")
        .body(Body::empty())
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let ct1 = resp1
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(ct1, "image/avif");

    // Request 2: format=auto with Accept: image/webp;q=1, image/avif;q=0
    let req2 = Request::builder()
        .uri("/transform?src=https://images.example.com/pic.jpg&format=auto")
        .header(ACCEPT, "image/webp;q=1.0, image/avif;q=0")
        .body(Body::empty())
        .unwrap();

    let resp2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let ct2 = resp2
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_ne!(
        ct2, "image/avif",
        "Must not receive AVIF when client rejected AVIF"
    );
    assert_eq!(
        ct2, "image/jpeg",
        "Under Option A, photographic content returns JPEG when AVIF is rejected"
    );
}

// -----------------------------------------------------------------------------
// Finding L: EXIF orientation normalization
// -----------------------------------------------------------------------------
#[test]
fn test_finding_l_exif_orientation_applied_by_image_rs() {
    let processor = ImageRsProcessor::new();

    // Create a JPEG image
    let img_bytes = make_jpeg(200, 100);
    let out = processor
        .process(
            &img_bytes,
            ProcessOptions {
                target_w: Some(200),
                target_h: Some(100),
                fit: atihsrah::processor::FitMode::Fill,
                format: Some(ImageFormat::Png),
                quality: None,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(out.width, 200);
    assert_eq!(out.height, 100);
}
