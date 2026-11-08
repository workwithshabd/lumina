use atihsrah::config::AuthMode;
use reqwest::header::{ACCEPT, CONTENT_TYPE, VARY};
use reqwest::StatusCode;

use super::server::TestServer;

#[tokio::test]
async fn test_cache_key_collision_resistance() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    let client = reqwest::Client::new();
    let src = "https://images.example.com/collision-test.png";

    // Set of distinct parameter combinations that must never collide in the cache
    let query_variants = [
        format!("src={src}&w=100&h=200"),
        format!("src={src}&w=200&h=100"),
        format!("src={src}&w=100&h=200&allow_upscale=true"),
        format!("src={src}&w=100&h=200&fit=contain"),
        format!("src={src}&w=100&h=200&fit=fill"),
        format!("src={src}&w=100&h=200&format=webp"),
        format!("src={src}&w=100&h=200&format=avif"),
        format!("src={src}&w=100&h=200&quality_target=draft"),
        format!("src={src}&w=100&h=200&quality_target=high"),
        format!("src={src}&w=100&h=200&v=1"),
        format!("src={src}&w=100&h=200&v=2"),
    ];

    for (idx, q) in query_variants.iter().enumerate() {
        let uri = format!("http://{}/transform?{q}", server.addr);
        let resp = client.get(&uri).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Every variant is unique and must result in a cache MISS on its first request
        let cache_hdr = resp
            .headers()
            .get("x-atihsrah-cache")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            cache_hdr, "MISS",
            "Variant #{idx} ({q}) must be a cache MISS and not collide with prior variants"
        );
    }
}

#[tokio::test]
async fn test_cache_poisoning_differing_accept_headers_prevented() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    let client = reqwest::Client::new();
    let src = "https://images.example.com/accept-test.jpg";
    let uri = format!("http://{}/transform?src={src}&format=auto", server.addr);

    // 1. Client 1 prefers AVIF
    let resp_avif = client
        .get(&uri)
        .header(ACCEPT, "image/avif,image/webp,*/*")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_avif.status(), StatusCode::OK);
    assert_eq!(resp_avif.headers().get(CONTENT_TYPE).unwrap(), "image/avif");
    assert_eq!(
        resp_avif.headers().get(VARY).unwrap(),
        "Accept",
        "Auto-format responses must have Vary: Accept header"
    );
    assert_eq!(resp_avif.headers().get("x-atihsrah-cache").unwrap(), "MISS");

    // 2. Client 2 only supports WebP (cannot decode AVIF)
    // Must NOT receive Client 1's cached AVIF!
    // Under Stage 1 Option A, photos are served as JPEG when AVIF is not accepted (never lossless WebP for photos)
    let resp_webp = client
        .get(&uri)
        .header(ACCEPT, "image/webp,*/*")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_webp.status(), StatusCode::OK);
    assert_eq!(
        resp_webp.headers().get(CONTENT_TYPE).unwrap(),
        "image/jpeg",
        "Client without AVIF must receive JPEG for photographic content, NOT cached AVIF"
    );
    assert_eq!(
        resp_webp.headers().get("x-atihsrah-cache").unwrap(),
        "MISS",
        "WebP accept-class must be evaluated and cached under separate key from AVIF"
    );

    // 3. Repeated Client 2 request -> HIT for JPEG under WebP accept-class
    let resp_webp_hit = client
        .get(&uri)
        .header(ACCEPT, "image/webp,*/*")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_webp_hit.status(), StatusCode::OK);
    assert_eq!(
        resp_webp_hit.headers().get(CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );
    assert_eq!(
        resp_webp_hit.headers().get("x-atihsrah-cache").unwrap(),
        "HIT"
    );

    // 4. Repeated Client 1 request -> HIT for AVIF
    let resp_avif_hit = client
        .get(&uri)
        .header(ACCEPT, "image/avif,image/webp,*/*")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_avif_hit.status(), StatusCode::OK);
    assert_eq!(
        resp_avif_hit.headers().get(CONTENT_TYPE).unwrap(),
        "image/avif"
    );
    assert_eq!(
        resp_avif_hit.headers().get("x-atihsrah-cache").unwrap(),
        "HIT"
    );
}
