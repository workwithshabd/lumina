use std::sync::Arc;

use atihsrah::config::AuthMode;
use reqwest::StatusCode;

use super::fixtures::*;
use super::server::TestServer;

#[tokio::test]
async fn test_memory_exhaustion_prevented_by_pre_decode_bounds_check() {
    let server = TestServer::start(
        AuthMode::Public,
        vec!["https://images.example.com"],
        Some(Arc::new(|url: &str| {
            // Source returns image that claims 6000x6000 pixels
            // 6000 * 6000 * 4 = 144MB of uncompressed memory per task.
            // Concurrently firing this would cause OOM if not blocked beforehand.
            if url.contains("huge-mem") {
                Ok(make_png_decompression_bomb(6000, 6000))
            } else {
                Ok(make_valid_png(32, 32))
            }
        })),
    )
    .await;

    let client = reqwest::Client::new();

    // Fire 20 concurrent requests with unique query strings
    let mut tasks = Vec::new();
    for i in 0..20 {
        let client_clone = client.clone();
        let target_url = format!("https://images.example.com/huge-mem-{i}.png");
        let uri = format!(
            "http://{}/transform?src={}&w=4000&h=4000",
            server.addr,
            percent_encoding::utf8_percent_encode(&target_url, percent_encoding::NON_ALPHANUMERIC)
        );

        tasks.push(tokio::spawn(async move {
            let resp = client_clone.get(&uri).send().await.unwrap();
            resp.status()
        }));
    }

    for task in tasks {
        let status = task.await.unwrap();
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "Huge dimension requests must be rejected at probe check with 422"
        );
    }
}

#[tokio::test]
async fn test_cache_amplification_bounded_by_byte_capacity() {
    let server = TestServer::start(
        AuthMode::Public,
        vec!["https://images.example.com"],
        Some(Arc::new(|_url: &str| {
            // Generates valid PNG images (~20KB)
            Ok(make_valid_png(200, 200))
        })),
    )
    .await;

    let client = reqwest::Client::new();

    // Send 30 requests with unique versions so every one is a distinct cache entry
    for i in 0..30 {
        let uri = format!(
            "http://{}/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=150&v={i}",
            server.addr
        );
        let resp = client.get(&uri).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Check that cache size in state remains strictly bounded
    let cache_size = server.state.cache.weighted_size();
    let max_allowed_bytes = 50 * 1024 * 1024; // 50MB configured in TestServer

    assert!(
        cache_size <= max_allowed_bytes,
        "Cache size ({cache_size} bytes) exceeded maximum byte capacity ({max_allowed_bytes} bytes)"
    );
}
