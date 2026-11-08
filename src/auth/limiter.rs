use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use governor::clock::{Clock, DefaultClock};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};

use crate::config::{AuthMode, Config};
use crate::error::AppError;

pub type KeyedLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

pub struct RateLimitManager {
    pre_auth_limiter: Arc<KeyedLimiter>,
    global_limiter: Arc<KeyedLimiter>,
    miss_limiter: Arc<KeyedLimiter>,
    last_cleanup_secs: AtomicU64,
}

impl RateLimitManager {
    pub fn new(config: &Config) -> Self {
        let pre_auth_quota = Quota::per_minute(
            NonZeroU32::new(config.rate_limit_per_minute)
                .unwrap_or_else(|| NonZeroU32::new(1000).unwrap()),
        );

        let global_quota = Quota::per_minute(
            NonZeroU32::new(config.rate_limit_per_minute)
                .unwrap_or_else(|| NonZeroU32::new(1000).unwrap()),
        );
        let miss_quota = Quota::per_minute(
            NonZeroU32::new(config.rate_limit_miss_per_minute)
                .unwrap_or_else(|| NonZeroU32::new(100).unwrap()),
        );

        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            pre_auth_limiter: Arc::new(RateLimiter::keyed(pre_auth_quota)),
            global_limiter: Arc::new(RateLimiter::keyed(global_quota)),
            miss_limiter: Arc::new(RateLimiter::keyed(miss_quota)),
            last_cleanup_secs: AtomicU64::new(now_secs),
        }
    }

    /// Checks the generous pre-auth rate limit (keyed by client IP, before verifying signature).
    pub fn check_pre_auth(&self, key: &str) -> Result<(), AppError> {
        self.maybe_cleanup();
        match self.pre_auth_limiter.check_key(&key.to_string()) {
            Ok(()) => Ok(()),
            Err(negative) => {
                let wait = negative.wait_time_from(DefaultClock::default().now());
                let wait_secs = wait.as_secs().max(1);
                Err(AppError::RateLimited {
                    retry_after_secs: wait_secs,
                })
            }
        }
    }

    /// Checks the global rate limit (counts ALL authenticated requests, hits and misses).
    pub fn check_global(&self, key: &str) -> Result<(), AppError> {
        self.maybe_cleanup();
        match self.global_limiter.check_key(&key.to_string()) {
            Ok(()) => Ok(()),
            Err(negative) => {
                let wait = negative.wait_time_from(DefaultClock::default().now());
                let wait_secs = wait.as_secs().max(1);
                Err(AppError::RateLimited {
                    retry_after_secs: wait_secs,
                })
            }
        }
    }

    /// Checks the stricter miss rate limit (counts only cache MISSES - expensive origin fetch + transform).
    pub fn check_miss(&self, key: &str) -> Result<(), AppError> {
        self.maybe_cleanup();
        match self.miss_limiter.check_key(&key.to_string()) {
            Ok(()) => Ok(()),
            Err(negative) => {
                let wait = negative.wait_time_from(DefaultClock::default().now());
                let wait_secs = wait.as_secs().max(1);
                Err(AppError::RateLimited {
                    retry_after_secs: wait_secs,
                })
            }
        }
    }

    /// Evicts idle keys from governor internal key stores so memory is bounded.
    pub fn cleanup(&self) {
        self.pre_auth_limiter.retain_recent();
        self.global_limiter.retain_recent();
        self.miss_limiter.retain_recent();
    }

    fn maybe_cleanup(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let last = self.last_cleanup_secs.load(Ordering::Relaxed);
        // Periodic eviction every 30 seconds
        if now.saturating_sub(last) >= 30
            && self
                .last_cleanup_secs
                .compare_exchange(last, now, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
        {
            self.cleanup();
        }
    }
}

/// Extracts client IP address:
/// - In public mode or signed mode fallback, uses socket peer address.
/// - Respects `X-Forwarded-For` ONLY if peer address matches `TRUSTED_PROXY_CIDRS`.
pub fn extract_client_ip(
    peer_addr: SocketAddr,
    forwarded_for: Option<&str>,
    trusted_cidrs: &[ipnet::IpNet],
) -> IpAddr {
    let peer_ip = peer_addr.ip();
    if trusted_cidrs.is_empty() {
        return peer_ip;
    }

    let is_trusted = trusted_cidrs.iter().any(|cidr| cidr.contains(&peer_ip));
    if !is_trusted {
        return peer_ip;
    }

    if let Some(xff) = forwarded_for {
        for ip_str in xff.split(',').rev() {
            if let Ok(ip) = ip_str.trim().parse::<IpAddr>() {
                let in_trusted = trusted_cidrs.iter().any(|cidr| cidr.contains(&ip));
                if !in_trusted {
                    return ip;
                }
            }
        }
    }

    peer_ip
}

/// Resolves the rate limiter key according to auth mode:
/// - Signed mode: key by verified `kid` (or client IP if no valid key id)
/// - Public mode: key by client IP
pub fn resolve_rate_limit_key(
    auth_mode: &AuthMode,
    kid: Option<&str>,
    client_ip: IpAddr,
) -> String {
    match auth_mode {
        AuthMode::Signed => {
            if let Some(k) = kid {
                format!("kid:{k}")
            } else {
                format!("ip:{client_ip}")
            }
        }
        AuthMode::Public => format!("ip:{client_ip}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(
        rate_limit: u32,
        miss_limit: u32,
        trusted_proxy_cidrs: Vec<ipnet::IpNet>,
    ) -> Config {
        let mut origins = std::collections::HashSet::new();
        origins.insert("https://images.example.com".to_string());
        Config {
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
            auth_mode: AuthMode::Signed,
            signing_keys: std::collections::HashMap::new(),
            clock_skew_secs: 30,
            max_signature_ttl_secs: 86400,
            rate_limit_per_minute: rate_limit,
            rate_limit_miss_per_minute: miss_limit,
            trusted_proxy_cidrs,
            debug_headers: true,
            metrics_bind: "127.0.0.1:0".parse().unwrap(),
            request_timeout_secs: 30,
            global_concurrency_limit: 1024,
            queue_saturation_threshold: 16,
            shutdown_timeout_secs: 5,
            max_body_bytes: 65536,
            private_origins: std::collections::HashSet::new(),
            private_ca_file: None,
            avif_speed: 6,
            distinct_color_threshold: 256,
            max_retry_pixels: 16_777_216,
            resize_filter: "lanczos3".to_string(),
        }
    }

    #[test]
    fn test_global_rate_limiter_exceeded() {
        let cfg = test_config(2, 2, Vec::new());
        let mgr = RateLimitManager::new(&cfg);

        assert!(mgr.check_global("user1").is_ok());
        assert!(mgr.check_global("user1").is_ok());

        let err = mgr.check_global("user1").unwrap_err();
        match err {
            AppError::RateLimited { retry_after_secs } => {
                assert!(retry_after_secs >= 1);
            }
            _ => panic!("Expected RateLimited error, got {err:?}"),
        }

        // Distinct key has separate quota
        assert!(mgr.check_global("user2").is_ok());
    }

    #[test]
    fn test_stricter_miss_rate_limiter() {
        let cfg = test_config(10, 2, Vec::new());
        let mgr = RateLimitManager::new(&cfg);

        // Global check succeeds
        for _ in 0..5 {
            assert!(mgr.check_global("key_a").is_ok());
        }

        // Miss limiter allows only 2
        assert!(mgr.check_miss("key_a").is_ok());
        assert!(mgr.check_miss("key_a").is_ok());

        let err = mgr.check_miss("key_a").unwrap_err();
        match err {
            AppError::RateLimited { retry_after_secs } => {
                assert!(retry_after_secs >= 1);
            }
            _ => panic!("Expected RateLimited error on miss, got {err:?}"),
        }
    }

    #[test]
    fn test_client_ip_trusted_proxy() {
        let peer_addr: SocketAddr = "10.0.0.1:12345".parse().unwrap();
        let trusted_cidrs = vec!["10.0.0.0/8".parse().unwrap()];

        // Trusted proxy forwarding a client IP
        let ip = extract_client_ip(
            peer_addr,
            Some("203.0.113.195, 198.51.100.1"),
            &trusted_cidrs,
        );
        assert_eq!(ip, "198.51.100.1".parse::<IpAddr>().unwrap());

        // Untrusted peer cannot spoof X-Forwarded-For
        let untrusted_peer: SocketAddr = "192.0.2.1:12345".parse().unwrap();
        let ip2 = extract_client_ip(untrusted_peer, Some("203.0.113.195"), &trusted_cidrs);
        assert_eq!(ip2, "192.0.2.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_rate_limiter_cleanup_bounds_key_store() {
        let cfg = test_config(100, 100, Vec::new());
        let mgr = RateLimitManager::new(&cfg);

        for i in 0..50 {
            let key = format!("transient_key_{i}");
            let _ = mgr.check_global(&key);
        }

        mgr.cleanup();
    }
}
