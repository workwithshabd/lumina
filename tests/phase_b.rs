use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::resolver::{HostLookup, SafeDnsResolver};
use atihsrah::fetch::SourceFetcher;
use reqwest::dns::Resolve;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone)]
struct MockLookup {
    addrs: HashMap<String, Vec<SocketAddr>>,
}

impl MockLookup {
    fn new() -> Self {
        Self {
            addrs: HashMap::new(),
        }
    }

    fn insert(&mut self, host: &str, addrs: Vec<SocketAddr>) {
        self.addrs.insert(host.to_string(), addrs);
    }
}

impl HostLookup for MockLookup {
    fn lookup(&self, host: &str, _port: u16) -> Result<Vec<SocketAddr>, std::io::Error> {
        if let Some(list) = self.addrs.get(host) {
            Ok(list.clone())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Host '{host}' not found in mock lookup"),
            ))
        }
    }
}

fn make_test_config(
    public_origins: &[&str],
    private_origins: &[&str],
    ca_file: Option<String>,
) -> Arc<Config> {
    let mut allowed = HashSet::new();
    for p in public_origins {
        allowed.insert(p.to_string());
    }
    let mut private = HashSet::new();
    for p in private_origins {
        private.insert(p.to_string());
    }

    Arc::new(Config {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        allowed_origins: allowed,
        source_fetch_timeout_secs: 10,
        source_connect_timeout_secs: 2,
        max_source_fetch_mb: 20,
        max_source_dimension: 8000,
        max_output_dimension: 4096,
        max_processing_memory_mb: 512,
        max_concurrent_transforms: 4,
        cache_max_size_mb: 500,
        cache_single_entry_max_mb: 10,
        cache_ttl_secs: 86400,
        auth_mode: AuthMode::Public,
        signing_keys: HashMap::new(),
        signing_key: None,
        clock_skew_secs: 30,
        max_signature_ttl_secs: 86400,
        rate_limit_per_minute: 1000,
        rate_limit_miss_per_minute: 100,
        trusted_proxy_cidrs: Vec::new(),
        log_level: "info".to_string(),
        debug_headers: false,
        metrics_bind: "127.0.0.1:9090".parse().unwrap(),
        request_timeout_secs: 30,
        global_concurrency_limit: 1024,
        queue_saturation_threshold: 16,
        shutdown_timeout_secs: 5,
        max_body_bytes: 65536,
        private_origins: private,
        private_ca_file: ca_file,
        avif_speed: 6,
        distinct_color_threshold: 256,
        max_retry_pixels: 16_777_216,
    })
}

// ---------------------------------------------------------------------------
// Test 1: Startup refuses http:// public origins in ALLOWED_ORIGINS, but
// accepts http:// in ATIHSRAH_PRIVATE_ORIGINS
// ---------------------------------------------------------------------------
#[test]
fn test_startup_refuses_http_public_origins() {
    let mut map = HashMap::new();
    map.insert(
        "ALLOWED_ORIGINS".to_string(),
        "http://public.example.com".to_string(),
    );
    let res = Config::from_lookup(|k| map.get(k).cloned());
    assert!(
        res.is_err(),
        "Public http:// origin must be rejected at startup"
    );

    // Valid configuration with public HTTPS and private HTTP
    let mut valid_map = HashMap::new();
    valid_map.insert("ATIHSRAH_AUTH_MODE".to_string(), "public".to_string());
    valid_map.insert(
        "ALLOWED_ORIGINS".to_string(),
        "https://images.unsplash.com".to_string(),
    );
    valid_map.insert(
        "ATIHSRAH_PRIVATE_ORIGINS".to_string(),
        "http://internal-minio:9000, https://internal-secure:8443".to_string(),
    );
    let cfg = Config::from_lookup(|k| valid_map.get(k).cloned()).expect("valid config");
    assert!(cfg.is_origin_allowed("https://images.unsplash.com"));
    assert!(cfg.is_origin_allowed("http://internal-minio:9000"));
    assert!(cfg.is_origin_allowed("https://internal-secure:8443"));

    // Invalid scheme in private origins rejected
    let mut invalid_private = HashMap::new();
    invalid_private.insert("ATIHSRAH_AUTH_MODE".to_string(), "public".to_string());
    invalid_private.insert(
        "ALLOWED_ORIGINS".to_string(),
        "https://images.unsplash.com".to_string(),
    );
    invalid_private.insert(
        "ATIHSRAH_PRIVATE_ORIGINS".to_string(),
        "ftp://internal:21".to_string(),
    );
    let err = Config::from_lookup(|k| invalid_private.get(k).cloned()).unwrap_err();
    assert!(format!("{err}").contains("Private origin scheme must be 'http' or 'https'"));
}

// ---------------------------------------------------------------------------
// Test 2: Listed private origin resolving to 10.x works
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_listed_private_origin_resolving_to_10_x_works() {
    let config = make_test_config(
        &["https://images.unsplash.com"],
        &["http://internal.service:8000"],
        None,
    );

    let mut mock_dns = MockLookup::new();
    mock_dns.insert("internal.service", vec!["10.0.0.5:8000".parse().unwrap()]);

    let private_hosts = config.private_origin_hosts();
    let resolver = SafeDnsResolver::new_with_private_hosts(mock_dns.clone(), private_hosts);

    // Resolving internal.service should succeed and return the 10.0.0.5 address
    let addrs = resolver
        .resolve("internal.service".parse().unwrap())
        .await
        .expect("Listed private origin resolving to 10.x must be allowed");
    let addr_vec: Vec<SocketAddr> = addrs.collect();
    assert_eq!(
        addr_vec,
        vec!["10.0.0.5:8000".parse::<SocketAddr>().unwrap()]
    );

    // Also verify live HTTP request execution against a local mock TCP server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_task = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let response =
                b"HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: 4\r\n\r\nTEST";
            let _ = socket.write_all(response).await;
        }
    });

    let live_config = make_test_config(
        &["https://images.unsplash.com"],
        &[&format!("http://internal.service:{port}")],
        None,
    );
    let mut live_mock = MockLookup::new();
    live_mock.insert(
        "internal.service",
        vec![SocketAddr::new("127.0.0.1".parse().unwrap(), port)],
    );

    let fetcher = SourceFetcher::new_with_lookup(live_config, live_mock).unwrap();
    let data = fetcher
        .fetch(&format!("http://internal.service:{port}/test.jpg"))
        .await
        .expect("Fetch to listed private origin must succeed");
    assert_eq!(data, b"TEST");

    server_task.abort();
}

// ---------------------------------------------------------------------------
// Test 3: Same IP via an unlisted hostname is blocked
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_same_ip_via_unlisted_hostname_is_blocked() {
    let config = make_test_config(
        &["https://public.service.com"],
        &["http://private.internal:8000"],
        None,
    );

    let mut mock_dns = MockLookup::new();
    // Both hosts resolve to the EXACT SAME 10.0.0.5 private IP
    mock_dns.insert("private.internal", vec!["10.0.0.5:443".parse().unwrap()]);
    mock_dns.insert("public.service.com", vec!["10.0.0.5:443".parse().unwrap()]);

    let private_hosts = config.private_origin_hosts();
    let resolver = SafeDnsResolver::new_with_private_hosts(mock_dns.clone(), private_hosts);

    // 1. Listed private origin resolves successfully to 10.0.0.5
    let priv_res = resolver.resolve("private.internal".parse().unwrap()).await;
    assert!(priv_res.is_ok(), "Listed private origin must be allowed");

    // 2. Unlisted/public hostname resolving to 10.0.0.5 is strictly blocked by SafeDnsResolver
    let pub_res = resolver
        .resolve("public.service.com".parse().unwrap())
        .await;
    assert!(
        pub_res.is_err(),
        "Public origin resolving to private IP 10.0.0.5 must be blocked"
    );

    // 3. SourceFetcher fetch returns SSRF_BLOCKED for public origin
    let fetcher = SourceFetcher::new_with_lookup(config, mock_dns).unwrap();
    let err = fetcher
        .fetch("https://public.service.com/pic.jpg")
        .await
        .unwrap_err();
    assert_eq!(err.code(), "SSRF_BLOCKED");
}

// ---------------------------------------------------------------------------
// Test 4: DNS rebinding from a listed private host to 169.254.169.254 is blocked
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_dns_rebinding_from_listed_private_host_to_metadata_is_blocked() {
    let config = make_test_config(
        &["https://images.unsplash.com"],
        &["http://private.internal:8000"],
        None,
    );

    let mut mock_dns = MockLookup::new();
    // Rebinding returns cloud metadata IP (169.254.169.254)
    mock_dns.insert(
        "private.internal",
        vec!["169.254.169.254:8000".parse().unwrap()],
    );

    let private_hosts = config.private_origin_hosts();
    let resolver = SafeDnsResolver::new_with_private_hosts(mock_dns.clone(), private_hosts);

    let res = resolver.resolve("private.internal".parse().unwrap()).await;
    assert!(
        res.is_err(),
        "Cloud metadata 169.254.169.254 MUST be blocked even for listed private origins"
    );

    let fetcher = SourceFetcher::new_with_lookup(config, mock_dns).unwrap();
    let err = fetcher
        .fetch("http://private.internal:8000/latest/meta-data")
        .await
        .unwrap_err();
    assert_eq!(err.code(), "SSRF_BLOCKED");
}

// ---------------------------------------------------------------------------
// Test 5: Invalid cert on a private https origin is rejected;
// trusted cert with ATIHSRAH_PRIVATE_CA_FILE succeeds.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_invalid_cert_on_private_https_origin_rejected() {
    // Pick an available port and launch Python mock origin with TLS enabled
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let cert_path = format!("{manifest_dir}/bench/certs/ca.crt");
    let script_path = format!("{manifest_dir}/bench/mock_origin.py");

    let py_process = std::process::Command::new("python3")
        .arg(&script_path)
        .arg(port.to_string())
        .arg("--tls")
        .spawn()
        .expect("Failed to start python mock origin server with TLS");

    struct ProcessGuard(std::process::Child);
    impl Drop for ProcessGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }
    let _guard = ProcessGuard(py_process);

    // Poll until mock origin is ready
    let target_origin = format!("https://127.0.0.1:{port}");
    let test_url = format!("{target_origin}/corpus/kodim01.png");
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Subtest A: Without ATIHSRAH_PRIVATE_CA_FILE, TLS validation fails
    let config_no_ca = make_test_config(
        &["https://images.unsplash.com"],
        &[&target_origin],
        None, // No private CA
    );
    let fetcher_no_ca = SourceFetcher::new(config_no_ca).unwrap();
    let res_no_ca = fetcher_no_ca.fetch(&test_url).await;
    assert!(
        res_no_ca.is_err(),
        "Self-signed/untrusted cert must be rejected when no private CA is configured"
    );
    let err_msg = format!("{}", res_no_ca.unwrap_err());
    assert!(
        err_msg.contains("Failed to connect")
            || err_msg.contains("certificate")
            || err_msg.contains("invalid cert")
            || err_msg.contains("UnknownIssuer"),
        "Expected TLS certificate rejection error, got: {err_msg}"
    );

    // Subtest B: With ATIHSRAH_PRIVATE_CA_FILE set to origin.crt, TLS validation succeeds
    let config_with_ca = make_test_config(
        &["https://images.unsplash.com"],
        &[&target_origin],
        Some(cert_path),
    );
    let fetcher_with_ca = SourceFetcher::new(config_with_ca).unwrap();
    let res_with_ca = fetcher_with_ca.fetch(&test_url).await;

    assert!(
        res_with_ca.is_ok(),
        "Fetch with valid private CA certificate must succeed, but failed with: {:?}",
        res_with_ca.err()
    );
}
