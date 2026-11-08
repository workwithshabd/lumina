pub mod limited_body;
pub mod resolver;
pub mod validator;

use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::error::AppError;
use crate::fetch::resolver::{HostLookup, SafeDnsResolver, SystemHostLookup};

pub type MockFetchFn = Arc<dyn Fn(&str) -> Result<Vec<u8>, AppError> + Send + Sync>;

#[derive(Clone)]
enum FetcherBackend {
    Network(reqwest::Client),
    Mock(MockFetchFn),
}

/// Single entrypoint for fetching external source images.
#[derive(Clone)]
pub struct SourceFetcher {
    backend: FetcherBackend,
    config: Arc<Config>,
}

impl SourceFetcher {
    /// Constructs a `SourceFetcher` using default system DNS resolution.
    pub fn new(config: Arc<Config>) -> Result<Self, AppError> {
        Self::new_with_lookup(config, SystemHostLookup)
    }

    /// Constructs a `SourceFetcher` with a custom host lookup provider (used for tests).
    pub fn new_with_lookup<L: HostLookup>(
        config: Arc<Config>,
        lookup: L,
    ) -> Result<Self, AppError> {
        let private_hosts = config.private_origin_hosts();
        let resolver = SafeDnsResolver::new_with_private_hosts(lookup, private_hosts);

        let mut builder = reqwest::Client::builder()
            .dns_resolver(Arc::new(resolver))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(config.source_connect_timeout_secs))
            .timeout(Duration::from_secs(config.source_fetch_timeout_secs))
            .no_proxy()
            .user_agent(format!("Atihsrah/{}", env!("CARGO_PKG_VERSION")))
            .use_rustls_tls();

        if let Some(ca_file) = &config.private_ca_file {
            let pem_bytes = std::fs::read(ca_file).map_err(|e| {
                AppError::Internal(format!(
                    "Failed to read ATIHSRAH_PRIVATE_CA_FILE '{ca_file}': {e}"
                ))
            })?;
            let certs = reqwest::Certificate::from_pem_bundle(&pem_bytes)
                .or_else(|_| reqwest::Certificate::from_pem(&pem_bytes).map(|c| vec![c]))
                .map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to parse ATIHSRAH_PRIVATE_CA_FILE '{ca_file}': {e}"
                    ))
                })?;
            for cert in certs {
                builder = builder.add_root_certificate(cert);
            }
            tracing::info!(ca_file = %ca_file, "Loaded private root CA certificate(s)");
        }

        let client = builder
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build HTTP client: {e}")))?;

        Ok(Self {
            backend: FetcherBackend::Network(client),
            config,
        })
    }

    /// Constructs a mock `SourceFetcher` for deterministic test suites.
    pub fn new_mock(config: Arc<Config>, mock_fn: MockFetchFn) -> Self {
        Self {
            backend: FetcherBackend::Mock(mock_fn),
            config,
        }
    }

    /// Fetches a source image by URL, enforcing all SSRF, allowlist, timeout, and size limits.
    pub async fn fetch(&self, raw_url: &str) -> Result<Vec<u8>, AppError> {
        // 1. Strict URL validation
        let validated_url = validator::validate_source_url_with_private(
            raw_url,
            &self.config.allowed_origins,
            &self.config.private_origins,
        )?;

        // 2. Compute sha256 of URL for sanitized correlation logging
        let url_hash = hash_url_for_log(raw_url);
        tracing::debug!(url_sha256 = %url_hash, "Initiating upstream image fetch");

        // 3. Dispatch to network client or mock backend
        let body = match &self.backend {
            FetcherBackend::Network(client) => {
                let response = client
                    .get(validated_url.as_str())
                    .send()
                    .await
                    .map_err(|e| {
                        if e.is_timeout() {
                            AppError::FetchTimeout
                        } else if is_ssrf_error(&e) {
                            AppError::SsrfBlocked(
                                "Resolved destination IP is private or restricted".to_string(),
                            )
                        } else if e.is_connect() {
                            AppError::FetchFailed(sanitize_reqwest_error(&e))
                        } else {
                            AppError::FetchFailed("Failed to execute request to origin".to_string())
                        }
                    })?;

                limited_body::read_limited_body(response, self.config.max_source_fetch_mb).await?
            }
            FetcherBackend::Mock(mock) => mock(raw_url)?,
        };

        tracing::debug!(
            url_sha256 = %url_hash,
            bytes_received = body.len(),
            "Upstream image fetch successful"
        );

        Ok(body)
    }
}

/// Checks if an error in reqwest's pipeline was triggered by our SSRF DNS resolver.
fn is_ssrf_error(err: &reqwest::Error) -> bool {
    let debug_repr = format!("{err:?}");
    if debug_repr.contains("SSRF protection blocked")
        || debug_repr.contains("no safe public IP addresses resolved")
        || debug_repr.contains("destination IP is restricted or cloud metadata")
    {
        return true;
    }

    let mut source = std::error::Error::source(err);
    while let Some(s) = source {
        let msg = s.to_string();
        if msg.contains("SSRF protection blocked")
            || msg.contains("no safe public IP addresses resolved")
            || msg.contains("destination IP is restricted or cloud metadata")
        {
            return true;
        }
        source = s.source();
    }
    false
}

/// Sanitizes reqwest errors to prevent URL, query parameter, or token leakage in error responses.
fn sanitize_reqwest_error(err: &reqwest::Error) -> String {
    let mut parts = Vec::new();
    let mut source = std::error::Error::source(err);
    while let Some(s) = source {
        let msg = s.to_string();
        if !msg.contains("for url (") {
            parts.push(msg);
        }
        source = s.source();
    }
    if parts.is_empty() {
        "Failed to connect to origin server".to_string()
    } else {
        format!("Failed to connect to origin: {}", parts.join(" -> "))
    }
}

/// Hashes a URL with SHA-256 for safe, non-sensitive audit logging.
pub fn hash_url_for_log(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    hex::encode(hasher.finalize())
}
