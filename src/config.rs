use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use thiserror::Error;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Signed,
    Public,
}

impl AuthMode {
    pub fn parse(s: &str) -> Result<Self, ConfigError> {
        match s.trim().to_lowercase().as_str() {
            "signed" => Ok(Self::Signed),
            "public" => Ok(Self::Public),
            other => Err(ConfigError::InvalidAuthMode(other.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub allowed_origins: HashSet<String>,
    pub source_fetch_timeout_secs: u64,
    pub source_connect_timeout_secs: u64,
    pub max_source_fetch_mb: usize,
    pub max_source_dimension: u32,
    pub max_output_dimension: u32,
    pub max_processing_memory_mb: usize,
    pub max_concurrent_transforms: usize,
    pub cache_max_size_mb: u64,
    pub cache_single_entry_max_mb: u64,
    pub cache_ttl_secs: u64,
    pub auth_mode: AuthMode,
    pub signing_keys: HashMap<String, Vec<u8>>,
    pub clock_skew_secs: u64,
    pub max_signature_ttl_secs: u64,
    pub rate_limit_per_minute: u32,
    pub rate_limit_miss_per_minute: u32,
    pub trusted_proxy_cidrs: Vec<ipnet::IpNet>,
    pub debug_headers: bool,
    pub metrics_bind: SocketAddr,
    pub request_timeout_secs: u64,
    pub global_concurrency_limit: usize,
    pub queue_saturation_threshold: usize,
    pub shutdown_timeout_secs: u64,
    pub max_body_bytes: usize,
    pub private_origins: HashSet<String>,
    pub private_ca_file: Option<String>,
    pub avif_speed: u8,
    pub distinct_color_threshold: u32,
    pub max_retry_pixels: u64,
    pub resize_filter: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("ALLOWED_ORIGINS is required and cannot be empty")]
    MissingAllowedOrigins,

    #[error("Invalid origin '{0}': {1}")]
    InvalidOrigin(String, String),

    #[error("Invalid auth mode '{0}': must be 'signed' or 'public'")]
    InvalidAuthMode(String),

    #[error("ATIHSRAH_SIGNING_KEY is required when ATIHSRAH_AUTH_MODE is 'signed'")]
    MissingSigningKey,

    #[error("Invalid ATIHSRAH_SIGNING_KEY: {0}")]
    InvalidSigningKey(String),

    #[error("Invalid value for '{0}': {1}")]
    InvalidValue(String, String),

    #[error("Configuration validation error: {0}")]
    Validation(String),
}

pub const VALID_ENV_VARS: &[&str] = &[
    "ALLOWED_ORIGINS",
    "ATIHSRAH_AUTH_MODE",
    "ATIHSRAH_AVIF_SPEED",
    "ATIHSRAH_BIND",
    "ATIHSRAH_CLOCK_SKEW_SECS",
    "ATIHSRAH_DEBUG_HEADERS",
    "ATIHSRAH_DISTINCT_COLOR_THRESHOLD",
    "ATIHSRAH_GLOBAL_CONCURRENCY_LIMIT",
    "ATIHSRAH_MAX_BODY_BYTES",
    "ATIHSRAH_MAX_RETRY_PIXELS",
    "ATIHSRAH_METRICS_BIND",
    "ATIHSRAH_PRIVATE_CA_FILE",
    "ATIHSRAH_PRIVATE_ORIGINS",
    "ATIHSRAH_QUEUE_SATURATION_THRESHOLD",
    "ATIHSRAH_REQUEST_TIMEOUT_SECS",
    "ATIHSRAH_SHUTDOWN_TIMEOUT_SECS",
    "ATIHSRAH_SIGNING_KEY",
    "ATIHSRAH_SIGNING_KEYS",
    "CACHE_MAX_SIZE_MB",
    "CACHE_SINGLE_ENTRY_MAX_MB",
    "CACHE_TTL_SECS",
    "LOG_LEVEL",
    "MAX_CONCURRENT_TRANSFORMS",
    "MAX_OUTPUT_DIMENSION",
    "MAX_PROCESSING_MEMORY_MB",
    "MAX_SIGNATURE_TTL_SECS",
    "MAX_SOURCE_DIMENSION",
    "MAX_SOURCE_FETCH_MB",
    "RATE_LIMIT_MISS_PER_MINUTE",
    "RATE_LIMIT_PER_MINUTE",
    "SOURCE_CONNECT_TIMEOUT_SECS",
    "SOURCE_FETCH_TIMEOUT_SECS",
    "TRUSTED_PROXY_CIDRS",
];

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut dp = vec![vec![0; n + 1]; m + 1];

    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate().take(n + 1) {
        *cell = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

pub fn find_closest_valid_env_var<'a>(candidate: &str, valid_vars: &[&'a str]) -> Option<&'a str> {
    valid_vars
        .iter()
        .min_by_key(|&&v| levenshtein(candidate, v))
        .copied()
}

/// Inspects a list of environment variable names. For any that look like our configuration
/// (starting with ATIHSRAH_ or close to a documented variable) but are unrecognized,
/// logs a warning suggesting the closest valid name.
///
/// Returns a list of `(unknown_var, suggested_name)`.
pub fn check_unknown_env_vars<I, S>(vars: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let valid_set: HashSet<&str> = VALID_ENV_VARS.iter().copied().collect();
    let mut warnings = Vec::new();

    for var in vars {
        let name = var.as_ref();
        if valid_set.contains(name) {
            continue;
        }

        let is_atihsrah_prefix =
            name.starts_with("ATIHSRAH_") || name.to_uppercase().starts_with("ATIHSRAH_");
        let closest = find_closest_valid_env_var(name, VALID_ENV_VARS);

        let should_warn = if is_atihsrah_prefix {
            true
        } else if let Some(c) = closest {
            // Documented variables without ATIHSRAH_ prefix (e.g. ALLOWED_ORIGINS, CACHE_MAX_SIZE_MB)
            // If edit distance <= 3, it's very likely an intended Atihsrah configuration
            levenshtein(name, c) <= 3
        } else {
            false
        };

        if should_warn {
            if let Some(closest_name) = closest {
                tracing::warn!(
                    unknown_var = %name,
                    suggestion = %closest_name,
                    "Unrecognized environment variable '{name}'. Did you mean '{closest_name}'?"
                );
                warnings.push((name.to_string(), closest_name.to_string()));
            }
        }
    }

    warnings
}

impl Config {
    /// Loads configuration from process environment and `.env` file if present.
    pub fn from_env() -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        let env_keys: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
        check_unknown_env_vars(&env_keys);
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Loads configuration from an arbitrary key lookup function (used for testing).
    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        // 1. Allowed Origins (Required, non-empty, strictly https)
        let origins_raw = lookup("ALLOWED_ORIGINS").ok_or(ConfigError::MissingAllowedOrigins)?;
        let allowed_origins = parse_allowed_origins(&origins_raw)?;

        // 2. Server Bind Address
        let bind_addr =
            parse_or_default(lookup("ATIHSRAH_BIND"), "127.0.0.1:8080", "ATIHSRAH_BIND")?;

        // 3. Timeouts & Limits
        let source_fetch_timeout_secs = parse_or_default(
            lookup("SOURCE_FETCH_TIMEOUT_SECS"),
            "15",
            "SOURCE_FETCH_TIMEOUT_SECS",
        )?;
        let source_connect_timeout_secs = parse_or_default(
            lookup("SOURCE_CONNECT_TIMEOUT_SECS"),
            "5",
            "SOURCE_CONNECT_TIMEOUT_SECS",
        )?;
        let max_source_fetch_mb =
            parse_or_default(lookup("MAX_SOURCE_FETCH_MB"), "20", "MAX_SOURCE_FETCH_MB")?;
        let max_source_dimension = parse_or_default(
            lookup("MAX_SOURCE_DIMENSION"),
            "8000",
            "MAX_SOURCE_DIMENSION",
        )?;
        let max_output_dimension = parse_or_default(
            lookup("MAX_OUTPUT_DIMENSION"),
            "4096",
            "MAX_OUTPUT_DIMENSION",
        )?;
        let max_processing_memory_mb = parse_or_default(
            lookup("MAX_PROCESSING_MEMORY_MB"),
            "512",
            "MAX_PROCESSING_MEMORY_MB",
        )?;
        let default_concurrency = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .to_string();
        let max_concurrent_transforms: usize = parse_or_default(
            lookup("MAX_CONCURRENT_TRANSFORMS"),
            &default_concurrency,
            "MAX_CONCURRENT_TRANSFORMS",
        )?;

        // 4. Cache
        let cache_max_size_mb =
            parse_or_default(lookup("CACHE_MAX_SIZE_MB"), "500", "CACHE_MAX_SIZE_MB")?;
        let cache_single_entry_max_mb = parse_or_default(
            lookup("CACHE_SINGLE_ENTRY_MAX_MB"),
            "10",
            "CACHE_SINGLE_ENTRY_MAX_MB",
        )?;
        let cache_ttl_secs = parse_or_default(lookup("CACHE_TTL_SECS"), "86400", "CACHE_TTL_SECS")?;

        // 5. Auth Mode & Signing Keys
        let auth_mode_raw = lookup("ATIHSRAH_AUTH_MODE").unwrap_or_else(|| "signed".to_string());
        let auth_mode = AuthMode::parse(&auth_mode_raw)?;

        let signing_keys = parse_signing_keys(
            lookup("ATIHSRAH_SIGNING_KEYS"),
            lookup("ATIHSRAH_SIGNING_KEY"),
        )?;

        if auth_mode == AuthMode::Signed && signing_keys.is_empty() {
            return Err(ConfigError::MissingSigningKey);
        }

        let clock_skew_secs = parse_or_default(
            lookup("ATIHSRAH_CLOCK_SKEW_SECS"),
            "30",
            "ATIHSRAH_CLOCK_SKEW_SECS",
        )?;
        let max_signature_ttl_secs = parse_or_default(
            lookup("MAX_SIGNATURE_TTL_SECS"),
            "86400",
            "MAX_SIGNATURE_TTL_SECS",
        )?;

        // 6. Rate Limit, Proxies & Logging
        let rate_limit_per_minute = parse_or_default(
            lookup("RATE_LIMIT_PER_MINUTE"),
            "1000",
            "RATE_LIMIT_PER_MINUTE",
        )?;
        let rate_limit_miss_per_minute = parse_or_default(
            lookup("RATE_LIMIT_MISS_PER_MINUTE"),
            "100",
            "RATE_LIMIT_MISS_PER_MINUTE",
        )?;
        let trusted_proxy_cidrs = parse_trusted_proxy_cidrs(lookup("TRUSTED_PROXY_CIDRS"))?;

        let debug_headers = parse_or_default(
            lookup("ATIHSRAH_DEBUG_HEADERS"),
            "false",
            "ATIHSRAH_DEBUG_HEADERS",
        )?;
        let metrics_bind = parse_or_default(
            lookup("ATIHSRAH_METRICS_BIND"),
            "127.0.0.1:9090",
            "ATIHSRAH_METRICS_BIND",
        )?;

        // 7. Hardening, Timeouts & Operational Limits
        let request_timeout_secs = parse_or_default(
            lookup("ATIHSRAH_REQUEST_TIMEOUT_SECS"),
            "30",
            "ATIHSRAH_REQUEST_TIMEOUT_SECS",
        )?;
        let global_concurrency_limit = parse_or_default(
            lookup("ATIHSRAH_GLOBAL_CONCURRENCY_LIMIT"),
            "1024",
            "ATIHSRAH_GLOBAL_CONCURRENCY_LIMIT",
        )?;
        let default_queue_threshold = (max_concurrent_transforms * 4).to_string();
        let queue_saturation_threshold = parse_or_default(
            lookup("ATIHSRAH_QUEUE_SATURATION_THRESHOLD"),
            &default_queue_threshold,
            "ATIHSRAH_QUEUE_SATURATION_THRESHOLD",
        )?;
        let shutdown_timeout_secs = parse_or_default(
            lookup("ATIHSRAH_SHUTDOWN_TIMEOUT_SECS"),
            "30",
            "ATIHSRAH_SHUTDOWN_TIMEOUT_SECS",
        )?;
        let max_body_bytes = parse_or_default(
            lookup("ATIHSRAH_MAX_BODY_BYTES"),
            "65536",
            "ATIHSRAH_MAX_BODY_BYTES",
        )?;
        let private_origins = parse_private_origins(lookup("ATIHSRAH_PRIVATE_ORIGINS"))?;
        let private_ca_file = lookup("ATIHSRAH_PRIVATE_CA_FILE").and_then(|s| {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        });

        // 8. Stage 1 Format & Heuristic Controls
        let avif_speed: u8 =
            parse_or_default(lookup("ATIHSRAH_AVIF_SPEED"), "6", "ATIHSRAH_AVIF_SPEED")?;
        if !(1..=10).contains(&avif_speed) {
            return Err(ConfigError::InvalidValue(
                "ATIHSRAH_AVIF_SPEED".to_string(),
                "must be between 1 and 10".to_string(),
            ));
        }
        let distinct_color_threshold: u32 = parse_or_default(
            lookup("ATIHSRAH_DISTINCT_COLOR_THRESHOLD"),
            "256",
            "ATIHSRAH_DISTINCT_COLOR_THRESHOLD",
        )?;
        let max_retry_pixels: u64 = parse_or_default(
            lookup("ATIHSRAH_MAX_RETRY_PIXELS"),
            "16777216",
            "ATIHSRAH_MAX_RETRY_PIXELS",
        )?;
        let resize_filter = lookup("ATIHSRAH_RESIZE_FILTER")
            .unwrap_or_else(|| "lanczos3".to_string())
            .to_lowercase();

        if !private_origins.is_empty() {
            let mut sorted: Vec<&String> = private_origins.iter().collect();
            sorted.sort();
            tracing::warn!(
                private_origins = ?sorted,
                "SECURITY WARNING: ATIHSRAH_PRIVATE_ORIGINS is enabled. Resolution to private addresses is permitted ONLY for listed origins: {}",
                sorted.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
        }

        // 8. Sanity validations
        if source_fetch_timeout_secs == 0 {
            return Err(ConfigError::Validation(
                "SOURCE_FETCH_TIMEOUT_SECS must be greater than 0".to_string(),
            ));
        }
        if source_connect_timeout_secs == 0 {
            return Err(ConfigError::Validation(
                "SOURCE_CONNECT_TIMEOUT_SECS must be greater than 0".to_string(),
            ));
        }
        if max_source_dimension == 0 || max_output_dimension == 0 {
            return Err(ConfigError::Validation(
                "MAX_SOURCE_DIMENSION and MAX_OUTPUT_DIMENSION must be greater than 0".to_string(),
            ));
        }
        if max_output_dimension > max_source_dimension {
            return Err(ConfigError::Validation(
                "MAX_OUTPUT_DIMENSION cannot exceed MAX_SOURCE_DIMENSION".to_string(),
            ));
        }
        if max_concurrent_transforms == 0 {
            return Err(ConfigError::Validation(
                "MAX_CONCURRENT_TRANSFORMS must be greater than 0".to_string(),
            ));
        }
        if cache_max_size_mb == 0 {
            return Err(ConfigError::Validation(
                "CACHE_MAX_SIZE_MB must be greater than 0".to_string(),
            ));
        }
        if cache_single_entry_max_mb > cache_max_size_mb {
            return Err(ConfigError::Validation(
                "CACHE_SINGLE_ENTRY_MAX_MB cannot exceed CACHE_MAX_SIZE_MB".to_string(),
            ));
        }
        if rate_limit_per_minute == 0 {
            return Err(ConfigError::Validation(
                "RATE_LIMIT_PER_MINUTE must be greater than 0".to_string(),
            ));
        }
        if rate_limit_miss_per_minute == 0 {
            return Err(ConfigError::Validation(
                "RATE_LIMIT_MISS_PER_MINUTE must be greater than 0".to_string(),
            ));
        }
        if request_timeout_secs == 0 {
            return Err(ConfigError::Validation(
                "ATIHSRAH_REQUEST_TIMEOUT_SECS must be greater than 0".to_string(),
            ));
        }
        if global_concurrency_limit == 0 {
            return Err(ConfigError::Validation(
                "ATIHSRAH_GLOBAL_CONCURRENCY_LIMIT must be greater than 0".to_string(),
            ));
        }
        if queue_saturation_threshold == 0 {
            return Err(ConfigError::Validation(
                "ATIHSRAH_QUEUE_SATURATION_THRESHOLD must be greater than 0".to_string(),
            ));
        }

        Ok(Config {
            bind_addr,
            allowed_origins,
            source_fetch_timeout_secs,
            source_connect_timeout_secs,
            max_source_fetch_mb,
            max_source_dimension,
            max_output_dimension,
            max_processing_memory_mb,
            max_concurrent_transforms,
            cache_max_size_mb,
            cache_single_entry_max_mb,
            cache_ttl_secs,
            auth_mode,
            signing_keys,
            clock_skew_secs,
            max_signature_ttl_secs,
            rate_limit_per_minute,
            rate_limit_miss_per_minute,
            trusted_proxy_cidrs,
            debug_headers,
            metrics_bind,
            request_timeout_secs,
            global_concurrency_limit,
            queue_saturation_threshold,
            shutdown_timeout_secs,
            max_body_bytes,
            private_origins,
            private_ca_file,
            avif_speed,
            distinct_color_threshold,
            max_retry_pixels,
            resize_filter,
        })
    }

    /// Returns the default signing key bytes, if configured.
    pub fn default_signing_key(&self) -> Option<&[u8]> {
        self.signing_keys
            .get("default")
            .or_else(|| self.signing_keys.values().next())
            .map(|v| v.as_slice())
    }

    /// Checks if a given origin is explicitly allowlisted (public or private).
    pub fn is_origin_allowed(&self, origin: &str) -> bool {
        let normalized = origin.trim_end_matches('/');
        self.allowed_origins.contains(normalized) || self.private_origins.contains(normalized)
    }

    /// Returns the set of hostnames that belong to private origins.
    pub fn private_origin_hosts(&self) -> HashSet<String> {
        self.private_origins
            .iter()
            .filter_map(|orig| {
                Url::parse(orig)
                    .ok()
                    .and_then(|u| u.host_str().map(|h| h.trim_end_matches('.').to_lowercase()))
            })
            .collect()
    }

    #[cfg(test)]
    pub fn test_default(origins: &[&str]) -> Self {
        let mut allowed = HashSet::new();
        for o in origins {
            allowed.insert(o.to_string());
        }
        Self {
            bind_addr: "127.0.0.1:8080".parse().unwrap(),
            allowed_origins: allowed,
            source_fetch_timeout_secs: 15,
            source_connect_timeout_secs: 5,
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
        }
    }
}

fn parse_allowed_origins(raw: &str) -> Result<HashSet<String>, ConfigError> {
    let mut origins = HashSet::new();
    for part in raw.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed = Url::parse(trimmed).map_err(|e| {
            ConfigError::InvalidOrigin(trimmed.to_string(), format!("Cannot parse URL: {e}"))
        })?;

        if parsed.scheme() != "https" {
            return Err(ConfigError::InvalidOrigin(
                trimmed.to_string(),
                "Only 'https' origins are permitted".to_string(),
            ));
        }

        if parsed.host_str().is_none() {
            return Err(ConfigError::InvalidOrigin(
                trimmed.to_string(),
                "Origin must have a valid host".to_string(),
            ));
        }

        let path = parsed.path();
        if !path.is_empty() && path != "/" {
            return Err(ConfigError::InvalidOrigin(
                trimmed.to_string(),
                "Origin must not contain a path".to_string(),
            ));
        }

        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(ConfigError::InvalidOrigin(
                trimmed.to_string(),
                "Origin must not contain query or fragment".to_string(),
            ));
        }

        // Canonical origin format: https://host or https://host:port
        let canonical = match parsed.port() {
            Some(p) => format!("https://{}:{p}", parsed.host_str().unwrap_or_default()),
            None => format!("https://{}", parsed.host_str().unwrap_or_default()),
        };
        origins.insert(canonical);
    }

    if origins.is_empty() {
        return Err(ConfigError::MissingAllowedOrigins);
    }

    Ok(origins)
}

fn parse_private_origins(raw: Option<String>) -> Result<HashSet<String>, ConfigError> {
    let mut origins = HashSet::new();
    let Some(raw) = raw else {
        return Ok(origins);
    };

    for part in raw.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed = Url::parse(trimmed).map_err(|e| {
            ConfigError::InvalidOrigin(trimmed.to_string(), format!("Cannot parse URL: {e}"))
        })?;

        let scheme = parsed.scheme();
        if scheme != "https" && scheme != "http" {
            return Err(ConfigError::InvalidOrigin(
                trimmed.to_string(),
                "Private origin scheme must be 'http' or 'https'".to_string(),
            ));
        }

        if parsed.host_str().is_none() {
            return Err(ConfigError::InvalidOrigin(
                trimmed.to_string(),
                "Origin must have a valid host".to_string(),
            ));
        }

        let path = parsed.path();
        if !path.is_empty() && path != "/" {
            return Err(ConfigError::InvalidOrigin(
                trimmed.to_string(),
                "Origin must not contain a path".to_string(),
            ));
        }

        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(ConfigError::InvalidOrigin(
                trimmed.to_string(),
                "Origin must not contain query or fragment".to_string(),
            ));
        }

        let host = parsed
            .host_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_lowercase();
        let canonical = match parsed.port() {
            Some(p) => {
                if (p == 80 && scheme == "http") || (p == 443 && scheme == "https") {
                    format!("{scheme}://{host}")
                } else {
                    format!("{scheme}://{host}:{p}")
                }
            }
            None => format!("{scheme}://{host}"),
        };
        origins.insert(canonical);
    }

    Ok(origins)
}

fn parse_or_default<T>(
    value_opt: Option<String>,
    default_str: &str,
    name: &str,
) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let s = value_opt.unwrap_or_else(|| default_str.to_string());
    s.trim()
        .parse::<T>()
        .map_err(|e| ConfigError::InvalidValue(name.to_string(), e.to_string()))
}

const DENYLISTED_SIGNING_KEYS: &[&str] = &[
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
    "d1ded29494fb8104468060d937eff6071508795ab923db667fe49b0cede896fd",
];

fn is_weak_or_denylisted_key(key_str: &str, decoded: &[u8]) -> bool {
    let lower = key_str.trim().to_lowercase();
    for denylisted in DENYLISTED_SIGNING_KEYS {
        if lower == *denylisted {
            return true;
        }
    }
    if decoded.iter().all(|&b| b == 0) {
        return true;
    }
    if decoded.iter().all(|&b| b == decoded[0]) {
        return true;
    }
    if lower
        .chars()
        .all(|c| c == lower.chars().next().unwrap_or('\0'))
    {
        return true;
    }
    false
}

fn parse_hex_key(key_str: &str) -> Result<Vec<u8>, ConfigError> {
    let trimmed = key_str.trim();
    let decoded = hex::decode(trimmed)
        .map_err(|e| ConfigError::InvalidSigningKey(format!("Failed to hex decode key: {e}")))?;
    if decoded.len() < 32 {
        return Err(ConfigError::InvalidSigningKey(format!(
            "Key length is {} bytes, minimum required is 32 bytes (256 bits)",
            decoded.len()
        )));
    }
    if is_weak_or_denylisted_key(trimmed, &decoded) {
        return Err(ConfigError::InvalidSigningKey(
            "Insecure or documented example signing key rejected. Refusing to start in signed mode with a documented example key, all-zero key, or repeated single-character key. Generate a cryptographically secure 32-byte key by running: openssl rand -hex 32".to_string(),
        ));
    }
    Ok(decoded)
}

fn parse_signing_keys(
    keys_raw: Option<String>,
    single_key_raw: Option<String>,
) -> Result<HashMap<String, Vec<u8>>, ConfigError> {
    let mut map = HashMap::new();

    if let Some(raw) = keys_raw {
        for entry in raw.split([',', '\n', ';']) {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let (kid, hex_part) = entry.split_once(':').ok_or_else(|| {
                ConfigError::InvalidSigningKey(format!(
                    "Invalid entry in ATIHSRAH_SIGNING_KEYS: expected 'kid:hexkey', got '{entry}'"
                ))
            })?;
            let kid = kid.trim();
            if kid.is_empty() {
                return Err(ConfigError::InvalidSigningKey(
                    "Key ID in ATIHSRAH_SIGNING_KEYS cannot be empty".to_string(),
                ));
            }
            let key = parse_hex_key(hex_part)?;
            map.insert(kid.to_string(), key);
        }
    }

    if let Some(single) = single_key_raw {
        let trimmed = single.trim();
        if !trimmed.is_empty() {
            let key = parse_hex_key(trimmed)?;
            map.entry("default".to_string()).or_insert(key);
        }
    }

    Ok(map)
}

fn parse_trusted_proxy_cidrs(raw: Option<String>) -> Result<Vec<ipnet::IpNet>, ConfigError> {
    let mut cidrs = Vec::new();
    if let Some(s) = raw {
        for part in s.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let cidr: ipnet::IpNet = part.parse().map_err(|e| {
                ConfigError::Validation(format!(
                    "Invalid CIDR '{part}' in TRUSTED_PROXY_CIDRS: {e}"
                ))
            })?;
            cidrs.push(cidr);
        }
    }
    Ok(cidrs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_test_map(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_missing_allowed_origins() {
        let map = make_test_map(&[]);
        let err = Config::from_lookup(|k| map.get(k).cloned()).unwrap_err();
        assert_eq!(err, ConfigError::MissingAllowedOrigins);
    }

    #[test]
    fn test_empty_allowed_origins() {
        let map = make_test_map(&[("ALLOWED_ORIGINS", "  ,  ")]);
        let err = Config::from_lookup(|k| map.get(k).cloned()).unwrap_err();
        assert_eq!(err, ConfigError::MissingAllowedOrigins);
    }

    #[test]
    fn test_http_origin_rejected() {
        let map = make_test_map(&[("ALLOWED_ORIGINS", "http://insecure.example.com")]);
        let err = Config::from_lookup(|k| map.get(k).cloned()).unwrap_err();
        match err {
            ConfigError::InvalidOrigin(origin, reason) => {
                assert_eq!(origin, "http://insecure.example.com");
                assert!(reason.contains("Only 'https' origins are permitted"));
            }
            other => panic!("Unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_origin_with_path_rejected() {
        let map = make_test_map(&[("ALLOWED_ORIGINS", "https://example.com/subpath")]);
        let err = Config::from_lookup(|k| map.get(k).cloned()).unwrap_err();
        match err {
            ConfigError::InvalidOrigin(_, reason) => {
                assert!(reason.contains("Origin must not contain a path"));
            }
            other => panic!("Unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_signed_mode_requires_signing_key() {
        let map = make_test_map(&[
            ("ALLOWED_ORIGINS", "https://images.example.com"),
            ("ATIHSRAH_AUTH_MODE", "signed"),
        ]);
        let err = Config::from_lookup(|k| map.get(k).cloned()).unwrap_err();
        assert_eq!(err, ConfigError::MissingSigningKey);
    }

    #[test]
    fn test_short_signing_key_rejected() {
        // 31 bytes (62 hex chars)
        let short_key = "00".repeat(31);
        let map = make_test_map(&[
            ("ALLOWED_ORIGINS", "https://images.example.com"),
            ("ATIHSRAH_SIGNING_KEY", &short_key),
        ]);
        let err = Config::from_lookup(|k| map.get(k).cloned()).unwrap_err();
        match err {
            ConfigError::InvalidSigningKey(reason) => {
                assert!(reason.contains("minimum required is 32 bytes"));
            }
            other => panic!("Unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_invalid_hex_signing_key_rejected() {
        let map = make_test_map(&[
            ("ALLOWED_ORIGINS", "https://images.example.com"),
            ("ATIHSRAH_SIGNING_KEY", "not-a-hex-string-and-too-short"),
        ]);
        let err = Config::from_lookup(|k| map.get(k).cloned()).unwrap_err();
        match err {
            ConfigError::InvalidSigningKey(reason) => {
                assert!(reason.contains("Failed to hex decode key"));
            }
            other => panic!("Unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_public_mode_does_not_require_signing_key() {
        let map = make_test_map(&[
            ("ALLOWED_ORIGINS", "https://images.example.com"),
            ("ATIHSRAH_AUTH_MODE", "public"),
        ]);
        let config = Config::from_lookup(|k| map.get(k).cloned()).unwrap();
        assert_eq!(config.auth_mode, AuthMode::Public);
        assert!(config.default_signing_key().is_none());
        assert!(config.signing_keys.is_empty());
    }

    #[test]
    fn test_valid_signed_mode_with_defaults() {
        let valid_key = "9f8e7d6c5b4a3928172635445566778899aabbccddeeff001122334455667788";
        let map = make_test_map(&[
            (
                "ALLOWED_ORIGINS",
                "https://images.example.com, https://cdn.example.org:8443/",
            ),
            ("ATIHSRAH_SIGNING_KEY", valid_key),
        ]);
        let config = Config::from_lookup(|k| map.get(k).cloned()).unwrap();
        assert_eq!(config.source_fetch_timeout_secs, 15);
        assert_eq!(config.source_connect_timeout_secs, 5);
        assert_eq!(config.max_source_fetch_mb, 20);
        assert_eq!(config.max_source_dimension, 8000);
        assert_eq!(config.max_output_dimension, 4096);
        assert_eq!(config.max_processing_memory_mb, 512);
        assert_eq!(config.cache_max_size_mb, 500);
        assert_eq!(config.cache_single_entry_max_mb, 10);
        assert_eq!(config.cache_ttl_secs, 86400);
        assert_eq!(config.rate_limit_per_minute, 1000);
        assert!(!config.debug_headers);
        assert!(config.is_origin_allowed("https://images.example.com"));
        assert!(config.is_origin_allowed("https://cdn.example.org:8443"));
        assert!(!config.is_origin_allowed("https://notallowed.com"));
        assert_eq!(config.default_signing_key().unwrap().len(), 32);
        assert_eq!(config.clock_skew_secs, 30);
        assert_eq!(config.max_signature_ttl_secs, 86400);
        assert_eq!(config.rate_limit_miss_per_minute, 100);
        assert!(config.trusted_proxy_cidrs.is_empty());
        assert_eq!(config.request_timeout_secs, 30);
        assert_eq!(config.global_concurrency_limit, 1024);
        assert_eq!(
            config.queue_saturation_threshold,
            config.max_concurrent_transforms * 4
        );
        assert_eq!(config.shutdown_timeout_secs, 30);
        assert_eq!(config.max_body_bytes, 65536);
    }

    #[test]
    fn test_multiple_signing_keys_rotation() {
        let key1 = "9f8e7d6c5b4a3928172635445566778899aabbccddeeff001122334455667788";
        let key2 = "7e6d5c4b3a291807f6e5d4c3b2a1908f7e6d5c4b3a291807f6e5d4c3b2a1908f";
        let keys_str = format!("k1:{key1},k2:{key2}");
        let map = make_test_map(&[
            ("ALLOWED_ORIGINS", "https://images.example.com"),
            ("ATIHSRAH_SIGNING_KEYS", &keys_str),
            ("TRUSTED_PROXY_CIDRS", "10.0.0.0/8, 192.168.1.0/24"),
            ("RATE_LIMIT_MISS_PER_MINUTE", "50"),
        ]);
        let config = Config::from_lookup(|k| map.get(k).cloned()).unwrap();
        assert_eq!(config.signing_keys.len(), 2);
        assert!(config.signing_keys.contains_key("k1"));
        assert!(config.signing_keys.contains_key("k2"));
        assert_eq!(config.rate_limit_miss_per_minute, 50);
        assert_eq!(config.trusted_proxy_cidrs.len(), 2);
    }

    #[test]
    fn test_startup_refuses_denylisted_and_weak_signing_keys() {
        let denylisted = [
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
            "d1ded29494fb8104468060d937eff6071508795ab923db667fe49b0cede896fd",
            // All zeros
            "0000000000000000000000000000000000000000000000000000000000000000",
            // Repeated single byte
            "1111111111111111111111111111111111111111111111111111111111111111",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ];

        for weak_key in denylisted {
            let map = make_test_map(&[
                ("ALLOWED_ORIGINS", "https://images.example.com"),
                ("ATIHSRAH_AUTH_MODE", "signed"),
                ("ATIHSRAH_SIGNING_KEY", weak_key),
            ]);
            let res = Config::from_lookup(|k| map.get(k).cloned());
            assert!(
                res.is_err(),
                "Expected weak/denylisted key '{weak_key}' to be rejected"
            );
            let err_msg = res.err().unwrap().to_string();
            assert!(
                err_msg.contains("openssl rand -hex 32"),
                "Expected error to advise running openssl rand -hex 32, got: {err_msg}"
            );

            // Also test via ATIHSRAH_SIGNING_KEYS
            let map_multi = make_test_map(&[
                ("ALLOWED_ORIGINS", "https://images.example.com"),
                ("ATIHSRAH_AUTH_MODE", "signed"),
                ("ATIHSRAH_SIGNING_KEYS", &format!("k1:{weak_key}")),
            ]);
            let res_multi = Config::from_lookup(|k| map_multi.get(k).cloned());
            assert!(
                res_multi.is_err(),
                "Expected key in ATIHSRAH_SIGNING_KEYS to be rejected"
            );
        }
    }
}
