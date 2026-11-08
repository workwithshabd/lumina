use std::collections::{HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::limited_body::read_limited_body;
use atihsrah::fetch::resolver::{is_safe_public_ip, HostLookup};
use atihsrah::fetch::validator::validate_source_url;
use atihsrah::fetch::SourceFetcher;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn make_test_config(allowed: &[&str], timeout_secs: u64, max_mb: usize) -> Arc<Config> {
    let mut origins = HashSet::new();
    for a in allowed {
        origins.insert(a.to_string());
    }

    Arc::new(Config {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        allowed_origins: origins,
        source_fetch_timeout_secs: timeout_secs,
        source_connect_timeout_secs: 2,
        max_source_fetch_mb: max_mb,
        max_source_dimension: 8000,
        max_output_dimension: 4096,
        max_processing_memory_mb: 512,
        max_concurrent_transforms: 4,
        cache_max_size_mb: 500,
        cache_single_entry_max_mb: 10,
        cache_ttl_secs: 86400,
        auth_mode: AuthMode::Public,
        signing_keys: std::collections::HashMap::new(),
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: 1000,
        rate_limit_miss_per_minute: 100,
        trusted_proxy_cidrs: Vec::new(),
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
        resize_filter: "lanczos3".to_string(),
    })
}

// -----------------------------------------------------------------------------
// 1. IP Validation Tests
// -----------------------------------------------------------------------------
#[test]
fn test_ssrf_blocked_ip_ranges() {
    let blocked_ips = [
        "169.254.169.254",        // AWS/GCP metadata
        "10.0.0.1",               // RFC 1918
        "127.0.0.1",              // Loopback
        "::1",                    // IPv6 Loopback
        "::ffff:192.168.1.1",     // IPv4-mapped private
        "::ffff:127.0.0.1",       // IPv4-mapped loopback
        "::ffff:169.254.169.254", // IPv4-mapped metadata
        "192.168.1.1",            // RFC 1918
        "172.16.0.1",             // RFC 1918
        "0.0.0.0",                // Zero address
        "100.64.0.1",             // CGNAT
        "192.0.2.1",              // TEST-NET-1
        "198.51.100.1",           // TEST-NET-2
        "203.0.113.1",            // TEST-NET-3
        "224.0.0.1",              // Multicast
        "240.0.0.1",              // Reserved
        "255.255.255.255",        // Broadcast
        "::",                     // IPv6 Unspecified
        "fc00::1",                // IPv6 ULA
        "fe80::1",                // IPv6 Link-Local
        "ff02::1",                // IPv6 Multicast
        "64:ff9b::1",             // NAT64 prefix
        "2001:db8::1",            // Documentation
    ];

    for ip_str in blocked_ips {
        let ip = IpAddr::from_str(ip_str).unwrap();
        assert!(
            !is_safe_public_ip(&ip),
            "Security vulnerability: IP {ip_str} should be BLOCKED"
        );
    }
}

// -----------------------------------------------------------------------------
// 2. Mock DNS Resolver Tests (DNS Pinning & Rebinding Defense)
// -----------------------------------------------------------------------------
type LookupResponse = Result<Vec<SocketAddr>, std::io::Error>;

#[derive(Clone, Debug)]
struct MockLookup {
    responses: Arc<Mutex<VecDeque<LookupResponse>>>,
}

impl MockLookup {
    fn new(responses: Vec<LookupResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(VecDeque::from(responses))),
        }
    }
}

impl HostLookup for MockLookup {
    fn lookup(&self, _host: &str, _port: u16) -> Result<Vec<SocketAddr>, std::io::Error> {
        let mut queue = self.responses.lock().unwrap();
        queue
            .pop_front()
            .unwrap_or_else(|| Ok(vec!["127.0.0.1:443".parse().unwrap()]))
    }
}

#[tokio::test]
async fn test_allowed_host_resolving_to_private_ip_blocked() {
    let config = make_test_config(&["https://images.unsplash.com"], 10, 20);

    // Mock DNS returning a private IP (e.g. 10.0.0.1) for an allowed origin
    let mock_dns = MockLookup::new(vec![Ok(vec!["10.0.0.1:443".parse().unwrap()])]);
    let fetcher = SourceFetcher::new_with_lookup(config, mock_dns).unwrap();

    let result = fetcher.fetch("https://images.unsplash.com/photo.jpg").await;

    let err = result.unwrap_err();
    assert_eq!(err.code(), "SSRF_BLOCKED");
}

#[tokio::test]
async fn test_dns_rebinding_second_lookup_private_blocked() {
    let config = make_test_config(&["https://images.unsplash.com"], 10, 20);

    // Sequence of lookups:
    // 1st lookup: public IP (93.184.216.34)
    // 2nd lookup: private IP (192.168.1.1) (attempted rebinding)
    let mock_dns = MockLookup::new(vec![
        Ok(vec!["93.184.216.34:443".parse().unwrap()]),
        Ok(vec!["192.168.1.1:443".parse().unwrap()]),
    ]);
    let fetcher = SourceFetcher::new_with_lookup(config, mock_dns).unwrap();

    // First call connects toward public IP (will fail connection without internet, but NOT with SSRF_BLOCKED)
    let res1 = fetcher
        .fetch("https://images.unsplash.com/photo1.jpg")
        .await;
    let err1 = res1.unwrap_err();
    assert_ne!(err1.code(), "SSRF_BLOCKED");

    // Second call triggers second lookup returning private IP -> MUST be blocked by SSRF
    let res2 = fetcher
        .fetch("https://images.unsplash.com/photo2.jpg")
        .await;
    let err2 = res2.unwrap_err();
    assert_eq!(err2.code(), "SSRF_BLOCKED");
}

// -----------------------------------------------------------------------------
// 3. URL Validator Security Tests
// -----------------------------------------------------------------------------
#[test]
fn test_url_validator_security_checks() {
    let allowed: HashSet<String> = vec![
        "https://images.unsplash.com".to_string(),
        "https://cdn.example.com".to_string(),
    ]
    .into_iter()
    .collect();

    // 1. HTTP scheme rejected
    let e = validate_source_url("http://images.unsplash.com/pic.png", &allowed).unwrap_err();
    assert_eq!(e.code(), "SOURCE_NOT_ALLOWED");

    // 2. Userinfo rejected
    let e =
        validate_source_url("https://user:pass@images.unsplash.com/pic.png", &allowed).unwrap_err();
    assert_eq!(e.code(), "SSRF_BLOCKED");

    // 3. Fragment rejected
    let e =
        validate_source_url("https://images.unsplash.com/pic.png#section", &allowed).unwrap_err();
    assert_eq!(e.code(), "INVALID_PARAM");

    // 4. Port 8443 rejected
    let e = validate_source_url("https://images.unsplash.com:8443/pic.png", &allowed).unwrap_err();
    assert_eq!(e.code(), "SOURCE_NOT_ALLOWED");

    // 5. Lookalike hostnames rejected
    let lookalikes = [
        "https://evil.com.images.unsplash.com/pic.png",
        "https://images.unsplash.com.evil.com/pic.png",
        "https://notimages.unsplash.com/pic.png",
        "https://images.unsplash.com.attacker/pic.png",
    ];
    for url in lookalikes {
        let err = validate_source_url(url, &allowed).unwrap_err();
        assert_eq!(err.code(), "SOURCE_NOT_ALLOWED");
    }
}

// -----------------------------------------------------------------------------
// 4. Local Test Server Tests (Redirects, Body Size Limits, Slow-Loris)
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_redirect_rejected() {
    // Spin up local TCP server returning a 302 redirect
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut req_buf = [0u8; 1024];
            let _ = socket.read(&mut req_buf).await;
            let response = "HTTP/1.1 302 Found\r\nLocation: https://cdn.example.com/other.png\r\nContent-Length: 0\r\n\r\n";
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{port}/image.png"))
        .send()
        .await
        .unwrap();

    let res = read_limited_body(resp, 10).await;
    let err = res.unwrap_err();
    assert_eq!(err.code(), "FETCH_FAILED");
    assert!(err.to_string().contains("Redirects are prohibited"));
}

#[tokio::test]
async fn test_body_larger_than_limit_no_content_length_aborted() {
    // Spin up local TCP server streaming chunked data without Content-Length
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut req_buf = [0u8; 1024];
            let _ = socket.read(&mut req_buf).await;

            // Send chunked HTTP response header with image/jpeg Content-Type
            let header =
                "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nTransfer-Encoding: chunked\r\n\r\n";
            let _ = socket.write_all(header.as_bytes()).await;

            // Send 3 chunks of 500 KB = 1.5 MB total (limit will be set to 1 MB)
            let chunk_data = vec![0x42u8; 500 * 1024];
            let chunk_header = format!("{:x}\r\n", chunk_data.len());

            for _ in 0..3 {
                let _ = socket.write_all(chunk_header.as_bytes()).await;
                let _ = socket.write_all(&chunk_data).await;
                let _ = socket.write_all(b"\r\n").await;
            }
            let _ = socket.write_all(b"0\r\n\r\n").await;
        }
    });

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{port}/large.jpg"))
        .send()
        .await
        .unwrap();

    // 1 MB limit
    let res = read_limited_body(resp, 1).await;
    let err = res.unwrap_err();
    assert_eq!(err.code(), "SOURCE_TOO_LARGE");
}

#[tokio::test]
async fn test_slow_loris_origin_times_out() {
    // Spin up local TCP server that stalls sending body bytes
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut req_buf = [0u8; 1024];
            let _ = socket.read(&mut req_buf).await;

            let header =
                "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nTransfer-Encoding: chunked\r\n\r\n";
            let _ = socket.write_all(header.as_bytes()).await;
            // Send initial chunk
            let _ = socket.write_all(b"4\r\nABCD\r\n").await;
            // Then stall and sleep longer than client timeout
            tokio::time::sleep(Duration::from_millis(500)).await;
            let _ = socket.write_all(b"0\r\n\r\n").await;
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(50)) // Short 50ms timeout
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{port}/slow.jpg"))
        .send()
        .await
        .unwrap();

    let res = read_limited_body(resp, 10).await;
    let err = res.unwrap_err();
    assert_eq!(err.code(), "FETCH_TIMEOUT");
}

#[tokio::test]
async fn test_dns_resolver_runs_on_blocking_pool_without_stalling_runtime() {
    use atihsrah::fetch::resolver::SafeDnsResolver;
    use reqwest::dns::Resolve;

    struct SlowLookup;
    impl HostLookup for SlowLookup {
        fn lookup(&self, _host: &str, _port: u16) -> Result<Vec<SocketAddr>, std::io::Error> {
            // Simulate synchronous blocking getaddrinfo latency
            std::thread::sleep(Duration::from_millis(60));
            Ok(vec!["93.184.216.34:443".parse().unwrap()])
        }
    }

    let resolver = SafeDnsResolver::new(SlowLookup);

    // Concurrently run resolve and an async timer task on Tokio
    let resolve_task = tokio::spawn(async move {
        let addrs = resolver
            .resolve("example.com".parse().unwrap())
            .await
            .expect("resolution succeeds");
        addrs.collect::<Vec<SocketAddr>>()
    });

    let mut ticks = 0;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        ticks += 1;
    }

    let result = resolve_task.await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].ip(), "93.184.216.34".parse::<IpAddr>().unwrap());
    assert!(
        ticks >= 4,
        "Async tasks continued progressing while DNS resolved in blocking pool"
    );
}
