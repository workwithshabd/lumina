use std::collections::HashSet;
use url::{Host, Url};

use crate::error::AppError;

pub const MAX_URL_LENGTH: usize = 4096;

/// Validates a source image URL against strict SSRF and allowlist constraints.
///
/// Rules enforced:
/// - Maximum URL length (4096 bytes)
/// - Scheme must be strictly HTTPS for public origins
/// - Userinfo (user:pass@) is strictly forbidden
/// - URL fragments (#...) are forbidden
/// - Port must be absent or exactly 443 for public origins
/// - IP literals are rejected with SsrfBlocked unless explicitly allowlisted
/// - Host is canonicalized (lowercased, punycode/IDNA, trailing dots stripped)
/// - Canonical origin must match ALLOWED_ORIGINS or ATIHSRAH_PRIVATE_ORIGINS exactly
pub fn validate_source_url(
    raw_url: &str,
    allowed_origins: &HashSet<String>,
) -> Result<Url, AppError> {
    static EMPTY_PRIVATE: std::sync::LazyLock<HashSet<String>> =
        std::sync::LazyLock::new(HashSet::new);
    validate_source_url_with_private(raw_url, allowed_origins, &EMPTY_PRIVATE)
}

/// Validates a source image URL against strict SSRF, public allowlist, and private origin constraints.
pub fn validate_source_url_with_private(
    raw_url: &str,
    allowed_origins: &HashSet<String>,
    private_origins: &HashSet<String>,
) -> Result<Url, AppError> {
    let trimmed = raw_url.trim();

    if trimmed.is_empty() {
        return Err(AppError::InvalidParam(
            "Source URL cannot be empty".to_string(),
        ));
    }

    if trimmed.len() > MAX_URL_LENGTH {
        return Err(AppError::InvalidParam(format!(
            "Source URL exceeds maximum allowed length of {MAX_URL_LENGTH} characters"
        )));
    }

    let parsed = Url::parse(trimmed)
        .map_err(|e| AppError::InvalidParam(format!("Invalid URL syntax: {e}")))?;

    // 1. Userinfo forbidden (credential stuffing / parser confusion vector)
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::SsrfBlocked(
            "Userinfo (credentials in URL) is forbidden".to_string(),
        ));
    }

    // 2. Fragments forbidden (not sent to HTTP servers, indicate confusion)
    if parsed.fragment().is_some() {
        return Err(AppError::InvalidParam(
            "URL fragments are not permitted in source URLs".to_string(),
        ));
    }

    // 3. Host validation and canonicalization
    let host = parsed.host().ok_or_else(|| {
        AppError::InvalidParam("Source URL must include a valid host".to_string())
    })?;

    let is_ip_literal = matches!(host, Host::Ipv4(_) | Host::Ipv6(_));

    // Normalize domain: strip trailing dot, ensure lowercase (Url::parse handles IDNA punycode)
    let raw_host_str = parsed.host_str().unwrap_or_default();
    let canonical_host = raw_host_str.trim_end_matches('.').to_lowercase();

    if canonical_host.is_empty() {
        return Err(AppError::InvalidParam(
            "Source URL host cannot be empty".to_string(),
        ));
    }

    let scheme = parsed.scheme();
    let canonical_origin = match parsed.port() {
        Some(port) => {
            if (port == 443 && scheme == "https") || (port == 80 && scheme == "http") {
                format!("{scheme}://{canonical_host}")
            } else {
                format!("{scheme}://{canonical_host}:{port}")
            }
        }
        None => format!("{scheme}://{canonical_host}"),
    };

    // 4. If host is an IP literal, reject unless explicitly allowlisted
    if is_ip_literal
        && !allowed_origins.contains(&canonical_origin)
        && !private_origins.contains(&canonical_origin)
    {
        return Err(AppError::SsrfBlocked(format!(
            "IP-literal host '{canonical_host}' is blocked"
        )));
    }

    // 5. Exact origin allowlist match (prevents suffix tricks like evil.com.allowed.com)
    let is_private_origin = private_origins.contains(&canonical_origin);
    let is_public_origin = allowed_origins.contains(&canonical_origin);

    if !is_private_origin && !is_public_origin {
        return Err(AppError::SourceNotAllowed(format!(
            "Origin '{canonical_origin}' is not in the allowlist"
        )));
    }

    // 6. Scheme and Port constraints
    if is_private_origin {
        if scheme != "http" && scheme != "https" {
            return Err(AppError::SourceNotAllowed(format!(
                "Invalid scheme '{scheme}': only 'http' or 'https' is permitted"
            )));
        }
    } else {
        // Public origins remain strictly HTTPS on port 443
        if scheme != "https" {
            return Err(AppError::SourceNotAllowed(format!(
                "Invalid scheme '{scheme}': public origins must use 'https'"
            )));
        }
        if let Some(port) = parsed.port() {
            if port != 443 {
                return Err(AppError::SourceNotAllowed(format!(
                    "Port '{port}' is not allowed: only standard HTTPS port 443 is permitted for public origins"
                )));
            }
        }
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_allowed() -> HashSet<String> {
        let mut set = HashSet::new();
        set.insert("https://images.unsplash.com".to_string());
        set.insert("https://cdn.example.com".to_string());
        set.insert("https://198.51.100.5".to_string()); // explicitly allowlisted public IP
        set
    }

    #[test]
    fn test_valid_urls() {
        let allowed = sample_allowed();

        let u1 =
            validate_source_url("https://images.unsplash.com/photo-12345?w=500", &allowed).unwrap();
        assert_eq!(u1.host_str(), Some("images.unsplash.com"));

        // Uppercase scheme and host canonicalization
        let u2 = validate_source_url("HTTPS://IMAGES.UNSPLASH.COM/photo-12345", &allowed).unwrap();
        assert_eq!(u2.host_str(), Some("images.unsplash.com"));

        // Trailing dot stripped
        let u3 = validate_source_url("https://images.unsplash.com./photo-12345", &allowed).unwrap();
        assert_eq!(u3.host_str(), Some("images.unsplash.com."));

        // Explicit 443 port allowed (Url parser normalizes default https port)
        let u4 =
            validate_source_url("https://images.unsplash.com:443/photo-12345", &allowed).unwrap();
        assert_eq!(u4.port_or_known_default(), Some(443));
    }

    #[test]
    fn test_reject_http_scheme() {
        let allowed = sample_allowed();
        let err =
            validate_source_url("http://images.unsplash.com/photo.jpg", &allowed).unwrap_err();
        assert_eq!(err.code(), "SOURCE_NOT_ALLOWED");
    }

    #[test]
    fn test_reject_userinfo() {
        let allowed = sample_allowed();
        let err = validate_source_url(
            "https://admin:secret@images.unsplash.com/photo.jpg",
            &allowed,
        )
        .unwrap_err();
        assert_eq!(err.code(), "SSRF_BLOCKED");
    }

    #[test]
    fn test_reject_fragment() {
        let allowed = sample_allowed();
        let err = validate_source_url("https://images.unsplash.com/photo.jpg#frag", &allowed)
            .unwrap_err();
        assert_eq!(err.code(), "INVALID_PARAM");
    }

    #[test]
    fn test_reject_non_443_port() {
        let allowed = sample_allowed();
        let err = validate_source_url("https://images.unsplash.com:8443/photo.jpg", &allowed)
            .unwrap_err();
        assert_eq!(err.code(), "SOURCE_NOT_ALLOWED");
    }

    #[test]
    fn test_reject_suffix_and_lookalike_tricks() {
        let allowed = sample_allowed();

        // evil.com.allowed.com
        let err1 = validate_source_url("https://evil.com.images.unsplash.com/photo.jpg", &allowed)
            .unwrap_err();
        assert_eq!(err1.code(), "SOURCE_NOT_ALLOWED");

        // allowed.com.evil.com
        let err2 = validate_source_url("https://images.unsplash.com.evil.com/photo.jpg", &allowed)
            .unwrap_err();
        assert_eq!(err2.code(), "SOURCE_NOT_ALLOWED");

        // Subdomain not in allowlist
        let err3 =
            validate_source_url("https://sub.images.unsplash.com/photo.jpg", &allowed).unwrap_err();
        assert_eq!(err3.code(), "SOURCE_NOT_ALLOWED");
    }

    #[test]
    fn test_reject_ip_literal_unless_allowlisted() {
        let allowed = sample_allowed();

        // Cloud metadata IP
        let err1 = validate_source_url("https://169.254.169.254/secret", &allowed).unwrap_err();
        assert_eq!(err1.code(), "SSRF_BLOCKED");

        // Loopback IPv4 and IPv6
        let err2 = validate_source_url("https://127.0.0.1/test.jpg", &allowed).unwrap_err();
        assert_eq!(err2.code(), "SSRF_BLOCKED");

        let err3 = validate_source_url("https://[::1]/test.jpg", &allowed).unwrap_err();
        assert_eq!(err3.code(), "SSRF_BLOCKED");

        // Allowlisted IP succeeds
        let ok = validate_source_url("https://198.51.100.5/test.jpg", &allowed);
        assert!(ok.is_ok());
    }

    #[test]
    fn test_max_url_length() {
        let allowed = sample_allowed();
        let long_path = "a".repeat(5000);
        let long_url = format!("https://images.unsplash.com/{long_path}");
        let err = validate_source_url(&long_url, &allowed).unwrap_err();
        assert_eq!(err.code(), "INVALID_PARAM");
    }
}
