use std::sync::Arc;

use atihsrah::config::AuthMode;
use reqwest::StatusCode;

use super::fixtures::*;
use super::server::{response_json, TestServer};

#[tokio::test]
async fn test_decompression_bombs_rejected_at_probe_before_decode() {
    let server = TestServer::start(
        AuthMode::Public,
        vec!["https://images.example.com"],
        Some(Arc::new(|url: &str| {
            if url.contains("png-bomb") {
                Ok(make_png_decompression_bomb(50000, 50000))
            } else if url.contains("jpeg-bomb") {
                Ok(make_jpeg_decompression_bomb(60000, 60000))
            } else if url.contains("webp-bomb") {
                Ok(make_webp_decompression_bomb(30000, 30000))
            } else {
                Ok(make_valid_png(64, 64))
            }
        })),
    )
    .await;

    let client = reqwest::Client::new();

    let bomb_cases = [
        ("https://images.example.com/png-bomb.png", "PNG bomb"),
        ("https://images.example.com/jpeg-bomb.jpg", "JPEG bomb"),
        ("https://images.example.com/webp-bomb.webp", "WebP bomb"),
    ];

    for (target_url, desc) in bomb_cases {
        let uri = format!(
            "http://{}/transform?src={}",
            server.addr,
            percent_encoding::utf8_percent_encode(target_url, percent_encoding::NON_ALPHANUMERIC)
        );
        let resp = client.get(&uri).send().await.unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{desc} claiming huge dimensions must be rejected with 422 Unprocessable Entity"
        );

        let json = response_json(resp).await;
        let code = json["code"].as_str().unwrap_or("");
        assert!(
            code == "IMAGE_TOO_LARGE" || code == "DECODE_FAILED",
            "Expected IMAGE_TOO_LARGE or DECODE_FAILED for {desc}, got {code}"
        );
    }
}

#[tokio::test]
async fn test_malformed_image_formats_rejected_with_structured_json() {
    let server = TestServer::start(
        AuthMode::Public,
        vec!["https://images.example.com"],
        Some(Arc::new(|url: &str| {
            if url.contains("corrupt.png") {
                Ok(make_malformed_png())
            } else if url.contains("corrupt.jpg") {
                Ok(make_malformed_jpeg())
            } else if url.contains("corrupt.webp") {
                Ok(make_malformed_webp())
            } else if url.contains("corrupt.avif") {
                Ok(make_malformed_avif())
            } else if url.contains("garbage.bin") {
                Ok(b"<html><body>Not an image</body></html>".to_vec())
            } else {
                Ok(make_valid_png(32, 32))
            }
        })),
    )
    .await;

    let client = reqwest::Client::new();

    let malformed_cases = [
        ("https://images.example.com/corrupt.png", "Malformed PNG"),
        ("https://images.example.com/corrupt.jpg", "Malformed JPEG"),
        ("https://images.example.com/corrupt.webp", "Malformed WebP"),
        ("https://images.example.com/corrupt.avif", "Malformed AVIF"),
        (
            "https://images.example.com/garbage.bin",
            "HTML/Garbage text",
        ),
    ];

    for (target_url, desc) in malformed_cases {
        let uri = format!(
            "http://{}/transform?src={}",
            server.addr,
            percent_encoding::utf8_percent_encode(target_url, percent_encoding::NON_ALPHANUMERIC)
        );
        let resp = client.get(&uri).send().await.unwrap();

        assert!(
            resp.status() == StatusCode::UNPROCESSABLE_ENTITY
                || resp.status() == StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{desc} must return 422 or 415, got status {}",
            resp.status()
        );

        let json = response_json(resp).await;
        assert!(json.get("error").is_some());
        assert!(json.get("code").is_some());
        assert!(json.get("request_id").is_some());
        assert!(json.get("hint").is_some());
    }
}
