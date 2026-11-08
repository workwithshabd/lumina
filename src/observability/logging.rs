use sha2::{Digest, Sha256};
use std::net::IpAddr;

/// Generates a redacted 16-hex-character SHA-256 prefix for a URL.
///
/// Ensures raw URLs (which may contain user tokens or sensitive origin paths)
/// are never logged in plaintext while still allowing correlation across log entries.
pub fn sha256_prefix(src: &str) -> String {
    let hash = hex::encode(Sha256::digest(src.as_bytes()));
    hash[..16].to_string()
}

/// Logs an HTTP access event under the "access" tracing target.
///
/// Contains request duration, status code, cache hit/miss result, and redacted URL hash.
#[allow(clippy::too_many_arguments)]
pub fn log_access(
    request_id: &str,
    method: &str,
    path: &str,
    status: u16,
    duration_ms: u128,
    cache_status: &str,
    src_sha256: Option<&str>,
    client_ip: IpAddr,
) {
    tracing::info!(
        target: "access",
        request_id = %request_id,
        method = %method,
        path = %path,
        status = status,
        duration_ms = duration_ms,
        cache_status = %cache_status,
        src_sha256 = src_sha256.unwrap_or("-"),
        client_ip = %client_ip,
        "HTTP request completed"
    );
}

/// Logs an application or system error under the "error" tracing target.
pub fn log_error(request_id: &str, code: &str, message: &str) {
    tracing::error!(
        target: "error",
        request_id = %request_id,
        code = %code,
        message = %message,
        "Request failed"
    );
}

/// Logs a security-relevant event (SSRF block, auth failure, rate limit) under the "security" tracing target.
///
/// Strictly omits raw keys, signatures, and full URLs.
pub fn log_security_event(
    request_id: &str,
    event_type: &str,
    reason: &str,
    src_sha256: Option<&str>,
    client_ip: Option<IpAddr>,
) {
    let ip_str = client_ip
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "-".to_string());

    tracing::warn!(
        target: "security",
        request_id = %request_id,
        event_type = %event_type,
        reason = %reason,
        src_sha256 = src_sha256.unwrap_or("-"),
        client_ip = %ip_str,
        "Security event detected"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_prefix_length_and_determinism() {
        let url1 = "https://images.example.com/sensitive-photo.jpg?token=secret123";
        let prefix1 = sha256_prefix(url1);
        let prefix2 = sha256_prefix(url1);

        assert_eq!(prefix1.len(), 16);
        assert_eq!(prefix1, prefix2);

        // Different URL produces different hash prefix
        let url2 = "https://images.example.com/other-photo.jpg";
        assert_ne!(prefix1, sha256_prefix(url2));
    }
}
