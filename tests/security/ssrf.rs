use std::sync::Arc;

use atihsrah::config::AuthMode;
use atihsrah::error::AppError;
use reqwest::StatusCode;

use super::server::{response_json, TestServer};

#[tokio::test]
async fn test_ssrf_all_ip_classes_and_ipv4_mapped_ipv6_blocked() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    let client = reqwest::Client::new();

    // Comprehensive list of blocked target IP representations
    let blocked_targets = [
        // IPv4 Loopback & unspecified
        "https://127.0.0.1/secret.png",
        "https://127.1.2.3/secret.png",
        "https://0.0.0.0/secret.png",
        // IPv4 Private RFC 1918
        "https://10.0.0.1/secret.png",
        "https://172.16.0.1/secret.png",
        "https://172.31.255.255/secret.png",
        "https://192.168.1.1/secret.png",
        // Cloud Metadata / Link-Local
        "https://169.254.169.254/latest/meta-data/",
        "https://169.254.1.1/secret.png",
        // Multicast & Broadcast
        "https://224.0.0.1/secret.png",
        "https://255.255.255.255/secret.png",
        // IPv6 Loopback & unspecified
        "https://[::1]/secret.png",
        "https://[::]/secret.png",
        // IPv6 Unique Local (fc00::/7)
        "https://[fd00::1]/secret.png",
        "https://[fc00::1]/secret.png",
        // IPv6 Link-Local
        "https://[fe80::1]/secret.png",
        // IPv4-mapped IPv6 (critical bypass vector)
        "https://[::ffff:127.0.0.1]/secret.png",
        "https://[::ffff:10.0.0.1]/secret.png",
        "https://[::ffff:169.254.169.254]/secret.png",
        "https://[::ffff:192.168.1.1]/secret.png",
    ];

    for target in blocked_targets {
        let uri = format!(
            "http://{}/transform?src={}",
            server.addr,
            urlencoding_lite(target)
        );
        let resp = client.get(&uri).send().await.unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Target {target} should be blocked with 403 Forbidden"
        );

        let json = response_json(resp).await;
        let code = json["code"].as_str().unwrap_or("");
        assert!(
            code == "SSRF_BLOCKED" || code == "SOURCE_NOT_ALLOWED",
            "Expected SSRF_BLOCKED or SOURCE_NOT_ALLOWED for target {target}, got {code}"
        );
    }
}

#[tokio::test]
async fn test_ssrf_dns_rebinding_blocked() {
    // Mock fetcher that simulates DNS rebinding: host looks allowlisted,
    // but the underlying DNS resolution attempts to return a private IP address.
    let server = TestServer::start(
        AuthMode::Public,
        vec!["https://images.example.com"],
        Some(Arc::new(|url: &str| {
            if url.contains("rebind.images.example.com") {
                // Simulate DNS rebinding to private IP
                Err(AppError::SsrfBlocked(
                    "DNS rebinding detected: host resolved to private IP 127.0.0.1".to_string(),
                ))
            } else {
                Ok(super::fixtures::make_valid_png(32, 32))
            }
        })),
    )
    .await;

    let client = reqwest::Client::new();
    let target = "https://images.example.com/rebind-test.png";

    // First request: regular fetch
    let uri1 = format!(
        "http://{}/transform?src={}",
        server.addr,
        urlencoding_lite(target)
    );
    let resp1 = client.get(&uri1).send().await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Second request: rebind to private address
    let rebind_target = "https://images.example.com/rebind.images.example.com/evil.png";
    let uri2 = format!(
        "http://{}/transform?src={}",
        server.addr,
        urlencoding_lite(rebind_target)
    );
    let resp2 = client.get(&uri2).send().await.unwrap();
    assert_eq!(resp2.status(), StatusCode::FORBIDDEN);
    let json2 = response_json(resp2).await;
    assert_eq!(json2["code"], "SSRF_BLOCKED");
}

#[tokio::test]
async fn test_redirect_rejection() {
    // When upstream origin returns HTTP 301/302/307/308 redirects,
    // Atihsrah must reject rather than following them.
    let server = TestServer::start(
        AuthMode::Public,
        vec!["https://images.example.com"],
        Some(Arc::new(|url: &str| {
            if url.contains("redirect-me") {
                Err(AppError::FetchFailed(
                    "HTTP redirect rejected: status 302 Found".to_string(),
                ))
            } else {
                Ok(super::fixtures::make_valid_png(32, 32))
            }
        })),
    )
    .await;

    let client = reqwest::Client::new();
    let target = "https://images.example.com/redirect-me.png";
    let uri = format!(
        "http://{}/transform?src={}",
        server.addr,
        urlencoding_lite(target)
    );

    let resp = client.get(&uri).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let json = response_json(resp).await;
    assert_eq!(json["code"], "FETCH_FAILED");
}

#[tokio::test]
async fn test_url_credentials_fragment_port_tricks_rejected() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    let client = reqwest::Client::new();

    let invalid_urls = [
        // Userinfo / credentials
        "https://admin:password@images.example.com/photo.jpg",
        "https://user@images.example.com/photo.jpg",
        // URL fragments
        "https://images.example.com/photo.jpg#frag",
        "https://images.example.com/photo.jpg#admin/delete",
        // Explicit non-443 ports
        "https://images.example.com:8080/photo.jpg",
        "https://images.example.com:80/photo.jpg",
        "https://images.example.com:8443/photo.jpg",
        // HTTP scheme
        "http://images.example.com/photo.jpg",
        // File scheme
        "file:///etc/passwd",
        // Gopher / FTP
        "gopher://images.example.com/",
        "ftp://images.example.com/photo.jpg",
    ];

    for target in invalid_urls {
        let uri = format!(
            "http://{}/transform?src={}",
            server.addr,
            urlencoding_lite(target)
        );
        let resp = client.get(&uri).send().await.unwrap();

        assert!(
            resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::FORBIDDEN,
            "Expected 400 or 403 for trick URL {target}, got {}",
            resp.status()
        );
    }
}

#[tokio::test]
async fn test_lookalike_hostnames_rejected() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    let client = reqwest::Client::new();

    let lookalikes = [
        // Suffix trick
        "https://images.example.com.attacker.com/photo.jpg",
        "https://images.example.com-evil.org/photo.jpg",
        // Subdomain prefix trick
        "https://evil.images.example.com/photo.jpg",
        // Path prefix trick
        "https://attacker.com/images.example.com/photo.jpg",
        // Query param trick
        "https://attacker.com/photo.jpg?origin=https://images.example.com",
    ];

    for target in lookalikes {
        let uri = format!(
            "http://{}/transform?src={}",
            server.addr,
            urlencoding_lite(target)
        );
        let resp = client.get(&uri).send().await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Lookalike hostname {target} must be rejected with 403 Forbidden"
        );
        let json = response_json(resp).await;
        assert_eq!(json["code"], "SOURCE_NOT_ALLOWED");
    }
}

fn urlencoding_lite(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}
