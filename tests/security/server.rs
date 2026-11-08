use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use atihsrah::auth::{build_canonical_string, compute_signature, rfc3986_encode};
use atihsrah::config::{AuthMode, Config};
use atihsrah::fetch::SourceFetcher;
use atihsrah::http::create_router;
use atihsrah::processor::image_rs::ImageRsProcessor;
use atihsrah::state::AppState;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub const TEST_SIGNING_KEY_HEX: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub type MockFetchCallback =
    Arc<dyn Fn(&str) -> Result<Vec<u8>, atihsrah::error::AppError> + Send + Sync>;

pub struct TestServer {
    pub addr: SocketAddr,
    pub state: Arc<AppState>,
    pub fetch_counter: Arc<AtomicUsize>,
    pub signing_key: Vec<u8>,
    _server_handle: JoinHandle<()>,
}

impl TestServer {
    pub async fn start(
        auth_mode: AuthMode,
        allowed_origins: Vec<&str>,
        mock_fetch: Option<MockFetchCallback>,
    ) -> Self {
        let raw_key = hex::decode(TEST_SIGNING_KEY_HEX).unwrap();
        let mut keys = HashMap::new();
        keys.insert("default".to_string(), raw_key.clone());

        let mut origins = HashSet::new();
        for o in allowed_origins {
            origins.insert(o.to_string());
        }

        let config = Arc::new(Config {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            allowed_origins: origins,
            source_fetch_timeout_secs: 5,
            source_connect_timeout_secs: 2,
            max_source_fetch_mb: 20,
            max_source_dimension: 8000,
            max_output_dimension: 4096,
            max_processing_memory_mb: 512,
            max_concurrent_transforms: 4,
            cache_max_size_mb: 50,
            cache_single_entry_max_mb: 5,
            cache_ttl_secs: 3600,
            auth_mode,
            signing_keys: keys,
            clock_skew_secs: 30,
            max_signature_ttl_secs: 86400,
            rate_limit_per_minute: 1000,
            rate_limit_miss_per_minute: 100,
            trusted_proxy_cidrs: Vec::new(),
            debug_headers: true,
            metrics_bind: "127.0.0.1:0".parse().unwrap(),
            request_timeout_secs: 10,
            global_concurrency_limit: 512,
            queue_saturation_threshold: 16,
            shutdown_timeout_secs: 5,
            max_body_bytes: 65536,
            private_origins: HashSet::new(),
            private_ca_file: None,
            avif_speed: 6,
            distinct_color_threshold: 256,
            max_retry_pixels: 16_777_216,
            resize_filter: "lanczos3".to_string(),
        });

        let fetch_counter = Arc::new(AtomicUsize::new(0));
        let fc = fetch_counter.clone();

        let fetcher = if let Some(cb) = mock_fetch {
            Arc::new(SourceFetcher::new_mock(config.clone(), cb))
        } else {
            Arc::new(SourceFetcher::new_mock(
                config.clone(),
                Arc::new(move |url: &str| {
                    fc.fetch_add(1, Ordering::SeqCst);
                    if url.ends_with(".jpg") || url.ends_with(".jpeg") {
                        Ok(super::fixtures::make_valid_jpeg(64, 64))
                    } else if url.ends_with(".webp") {
                        Ok(super::fixtures::make_valid_webp(64, 64))
                    } else {
                        Ok(super::fixtures::make_valid_png(64, 64))
                    }
                }),
            ))
        };

        let processor = Arc::new(ImageRsProcessor::new());
        let state = Arc::new(AppState::new_with_fetcher(config, processor, fetcher));
        state.set_ready(true);

        let router = create_router(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        // Small wait to ensure listener is accepting
        tokio::time::sleep(Duration::from_millis(20)).await;

        Self {
            addr,
            state,
            fetch_counter,
            signing_key: raw_key,
            _server_handle: server_handle,
        }
    }

    /// Helper to sign parameters and return full query string
    pub fn sign_params(&self, exp: u64, params: &[(String, String)]) -> String {
        let canonical = build_canonical_string(exp, params);
        let sig = compute_signature(&self.signing_key, &canonical);

        let mut parts = Vec::new();
        for (k, v) in params {
            parts.push(format!("{}={}", k, rfc3986_encode(v)));
        }
        parts.push(format!("sig={sig}"));
        parts.join("&")
    }

    pub fn now_exp(&self, delta_secs: i64) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if delta_secs >= 0 {
            now + delta_secs as u64
        } else {
            now.saturating_sub((-delta_secs) as u64)
        }
    }
}

pub async fn response_json(resp: reqwest::Response) -> serde_json::Value {
    let body = resp.text().await.expect("Failed to read response body");
    serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("Failed to parse response body as JSON: '{body}' - {e}"))
}
