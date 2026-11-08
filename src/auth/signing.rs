use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::config::Config;
use crate::error::AppError;

type HmacSha256 = Hmac<Sha256>;

/// RFC 3986 Unreserved characters set: `[A-Za-z0-9\-_.~]`.
///
/// All characters outside this set must be percent-encoded as uppercase hex `%XX`.
pub const RFC3986_UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Strictly percent-encodes a string according to RFC 3986.
pub fn rfc3986_encode(val: &str) -> String {
    utf8_percent_encode(val, RFC3986_UNRESERVED).to_string()
}

/// Constructs the canonical string representation for HMAC-SHA256 signing.
///
/// Canonical string format:
/// ```text
/// GET\n
/// /transform\n
/// {exp}\n
/// {param1_key}={param1_val}\n
/// ...
/// {paramN_key}={paramN_val}
/// ```
///
/// Rules:
/// - Parameters are sorted alphabetically by key (byte/ASCII ordering).
/// - The `sig` parameter is strictly excluded.
/// - Each parameter is serialized as `key=percent_encoded(value)`.
/// - Parameters are joined with `\n`.
pub fn build_canonical_string(exp: u64, params: &[(String, String)]) -> String {
    let exp_str = exp.to_string();

    // Filter out `sig`
    let mut filtered: Vec<(String, String)> = params
        .iter()
        .filter(|(k, _)| k != "sig")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Ensure `exp` is present in the parameter list if not already supplied
    if !filtered.iter().any(|(k, _)| k == "exp") {
        filtered.push(("exp".to_string(), exp_str.clone()));
    }

    // Sort parameters strictly by key (ASCII / byte comparison)
    filtered.sort_by(|a, b| a.0.cmp(&b.0));

    let mut canonical = format!("GET\n/transform\n{exp_str}\n");

    let params_joined: Vec<String> = filtered
        .iter()
        .map(|(k, v)| format!("{}={}", k, rfc3986_encode(v)))
        .collect();

    canonical.push_str(&params_joined.join("\n"));
    canonical
}

/// Computes the HMAC-SHA256 signature (lowercase hex string) for a canonical string.
pub fn compute_signature(key: &[u8], canonical: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(key).expect("HMAC can take key of any length >= 32 bytes");
    mac.update(canonical.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Verifies the signature of an incoming query string against server configuration.
///
/// On success, returns `Ok(Some(kid))` or `Ok(None)` indicating the active key ID used.
pub fn verify_signature(
    config: &Config,
    raw_query: &str,
    now_secs: u64,
) -> Result<Option<String>, AppError> {
    if raw_query.is_empty() {
        return Err(AppError::Unauthorized(
            "Missing required query parameters and signature".to_string(),
        ));
    }

    let mut sig: Option<String> = None;
    let mut exp: Option<u64> = None;
    let mut kid: Option<String> = None;
    let mut params_list = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();

    for pair in raw_query.split('&') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }

        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let raw_val = parts.next().unwrap_or("").trim();

        if key.is_empty() {
            continue;
        }

        // Strict duplicate parameter check
        if !seen_keys.insert(key.to_string()) {
            return Err(AppError::DuplicateParam(key.to_string()));
        }

        // Decode URL-encoded incoming value so we can canonicalize consistently
        let decoded_val = percent_encoding::percent_decode_str(raw_val)
            .decode_utf8()
            .map_err(|e| {
                AppError::InvalidParam(format!("Invalid URL encoding in parameter '{key}': {e}"))
            })?
            .to_string();

        match key {
            "sig" => sig = Some(decoded_val),
            "exp" => {
                let parsed: u64 = decoded_val.parse().map_err(|_| {
                    AppError::InvalidParam(
                        "Parameter 'exp' must be a valid unsigned integer timestamp".to_string(),
                    )
                })?;
                exp = Some(parsed);
                params_list.push((key.to_string(), decoded_val));
            }
            "kid" => {
                kid = Some(decoded_val.clone());
                params_list.push((key.to_string(), decoded_val));
            }
            other => {
                params_list.push((other.to_string(), decoded_val));
            }
        }
    }

    // 1. Signature parameter check
    let sig_str = sig.ok_or_else(|| {
        AppError::Unauthorized("Missing required signature parameter 'sig'".to_string())
    })?;

    // 2. Expiration timestamp check
    let exp_val = exp.ok_or_else(|| {
        AppError::Unauthorized("Missing required expiration parameter 'exp'".to_string())
    })?;

    // 3. Reject if expired in the past (respecting clock skew)
    if now_secs > exp_val + config.clock_skew_secs {
        return Err(AppError::Expired(format!(
            "Signature expired at timestamp {exp_val} (server time: {now_secs})"
        )));
    }

    // 4. Reject if far-future expiration beyond MAX_SIGNATURE_TTL_SECS
    if exp_val > now_secs + config.max_signature_ttl_secs + config.clock_skew_secs {
        return Err(AppError::Unauthorized(format!(
            "Expiration timestamp {exp_val} exceeds maximum allowed TTL of {} seconds",
            config.max_signature_ttl_secs
        )));
    }

    // 5. Select active signing key
    let (active_kid, key_bytes) = match &kid {
        Some(k) => {
            let key = config
                .signing_keys
                .get(k)
                .ok_or_else(|| AppError::Unauthorized(format!("Unknown signing key ID: '{k}'")))?;
            (Some(k.clone()), key.as_slice())
        }
        None => {
            if config.signing_keys.len() == 1 {
                let (k, key) = config.signing_keys.iter().next().unwrap();
                (Some(k.clone()), key.as_slice())
            } else if let Some(key) = config.signing_keys.get("default") {
                (Some("default".to_string()), key.as_slice())
            } else if config.signing_keys.is_empty() {
                return Err(AppError::Unauthorized(
                    "No signing keys configured on server".to_string(),
                ));
            } else {
                return Err(AppError::Unauthorized(
                    "Multiple signing keys configured; 'kid' parameter is required to identify key"
                        .to_string(),
                ));
            }
        }
    };

    // 6. Build canonical string from parameters (excluding sig)
    let canonical = build_canonical_string(exp_val, &params_list);

    // 7. Verify HMAC signature in constant time
    let mut mac =
        HmacSha256::new_from_slice(key_bytes).expect("HMAC key initialized with valid length");
    mac.update(canonical.as_bytes());
    let expected_mac = mac.finalize().into_bytes();

    let sig_bytes = hex::decode(sig_str.trim())
        .map_err(|_| AppError::Unauthorized("Signature 'sig' must be valid hex".to_string()))?;

    if !bool::from(sig_bytes.as_slice().ct_eq(expected_mac.as_slice())) {
        return Err(AppError::Unauthorized(
            "HMAC signature verification failed".to_string(),
        ));
    }

    Ok(active_kid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config(key_hex: &str) -> Config {
        let key_bytes = hex::decode(key_hex).unwrap();
        let mut signing_keys = HashMap::new();
        signing_keys.insert("default".to_string(), key_bytes.clone());
        let mut cfg = Config::test_default(&["https://images.example.com"]);
        cfg.auth_mode = crate::config::AuthMode::Signed;
        cfg.signing_keys = signing_keys;
        cfg.signing_key = Some(key_bytes);
        cfg
    }

    #[test]
    fn test_rfc3986_encoding() {
        assert_eq!(
            rfc3986_encode("https://example.com/test-image_1.jpg~beta"),
            "https%3A%2F%2Fexample.com%2Ftest-image_1.jpg~beta"
        );
        assert_eq!(rfc3986_encode("hello world!"), "hello%20world%21");
    }

    #[test]
    fn test_canonical_string_ordering() {
        let exp = 1700000000;
        let params = vec![
            ("w".to_string(), "300".to_string()),
            (
                "src".to_string(),
                "https://images.example.com/pic.jpg".to_string(),
            ),
            ("format".to_string(), "webp".to_string()),
            ("sig".to_string(), "to_be_ignored".to_string()),
        ];

        let canonical = build_canonical_string(exp, &params);
        let expected = "GET\n/transform\n1700000000\nexp=1700000000\nformat=webp\nsrc=https%3A%2F%2Fimages.example.com%2Fpic.jpg\nw=300";
        assert_eq!(canonical, expected);
    }

    #[test]
    fn test_verify_valid_signature_and_reordered_params() {
        let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let config = test_config(key_hex);
        let now = 1700000000;
        let exp = now + 3600;

        let params = vec![
            (
                "src".to_string(),
                "https://images.example.com/cat.jpg".to_string(),
            ),
            ("w".to_string(), "400".to_string()),
            ("exp".to_string(), exp.to_string()),
        ];

        let canonical = build_canonical_string(exp, &params);
        let sig = compute_signature(&hex::decode(key_hex).unwrap(), &canonical);

        // Query with order 1: src, w, exp, sig
        let query1 =
            format!("src=https%3A%2F%2Fimages.example.com%2Fcat.jpg&w=400&exp={exp}&sig={sig}");
        let res1 = verify_signature(&config, &query1, now);
        assert!(res1.is_ok());

        // Query with reordered params: w, exp, src, sig
        let query2 =
            format!("w=400&exp={exp}&src=https%3A%2F%2Fimages.example.com%2Fcat.jpg&sig={sig}");
        let res2 = verify_signature(&config, &query2, now);
        assert!(res2.is_ok());
    }

    #[test]
    fn test_verify_tampered_param() {
        let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let config = test_config(key_hex);
        let now = 1700000000;
        let exp = now + 3600;

        let params = vec![
            (
                "src".to_string(),
                "https://images.example.com/cat.jpg".to_string(),
            ),
            ("w".to_string(), "400".to_string()),
            ("exp".to_string(), exp.to_string()),
        ];
        let canonical = build_canonical_string(exp, &params);
        let sig = compute_signature(&hex::decode(key_hex).unwrap(), &canonical);

        // Tamper w from 400 to 500
        let tampered =
            format!("src=https%3A%2F%2Fimages.example.com%2Fcat.jpg&w=500&exp={exp}&sig={sig}");
        let res = verify_signature(&config, &tampered, now);
        assert!(matches!(res, Err(AppError::Unauthorized(_))));
    }

    #[test]
    fn test_verify_expired_signature() {
        let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let config = test_config(key_hex);
        let now = 1700000000;
        let exp = now - 60; // Expired 60s ago (> 30s clock skew)

        let params = vec![
            (
                "src".to_string(),
                "https://images.example.com/cat.jpg".to_string(),
            ),
            ("exp".to_string(), exp.to_string()),
        ];
        let canonical = build_canonical_string(exp, &params);
        let sig = compute_signature(&hex::decode(key_hex).unwrap(), &canonical);

        let query = format!("src=https%3A%2F%2Fimages.example.com%2Fcat.jpg&exp={exp}&sig={sig}");
        let res = verify_signature(&config, &query, now);
        assert!(matches!(res, Err(AppError::Expired(_))));
    }

    #[test]
    fn test_verify_clock_skew_tolerance() {
        let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let config = test_config(key_hex);
        let now = 1700000000;
        let exp = now - 20; // Expired 20s ago, within 30s tolerance

        let params = vec![
            (
                "src".to_string(),
                "https://images.example.com/cat.jpg".to_string(),
            ),
            ("exp".to_string(), exp.to_string()),
        ];
        let canonical = build_canonical_string(exp, &params);
        let sig = compute_signature(&hex::decode(key_hex).unwrap(), &canonical);

        let query = format!("src=https%3A%2F%2Fimages.example.com%2Fcat.jpg&exp={exp}&sig={sig}");
        let res = verify_signature(&config, &query, now);
        assert!(res.is_ok());
    }

    #[test]
    fn test_verify_far_future_expiry_rejected() {
        let key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let config = test_config(key_hex);
        let now = 1700000000;
        let exp = now + 90000; // > 86400s MAX_SIGNATURE_TTL_SECS

        let params = vec![
            (
                "src".to_string(),
                "https://images.example.com/cat.jpg".to_string(),
            ),
            ("exp".to_string(), exp.to_string()),
        ];
        let canonical = build_canonical_string(exp, &params);
        let sig = compute_signature(&hex::decode(key_hex).unwrap(), &canonical);

        let query = format!("src=https%3A%2F%2Fimages.example.com%2Fcat.jpg&exp={exp}&sig={sig}");
        let res = verify_signature(&config, &query, now);
        assert!(matches!(res, Err(AppError::Unauthorized(_))));
    }

    #[test]
    fn test_key_rotation_with_kid() {
        let key1 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let key2 = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        let mut config = test_config(key1);
        config
            .signing_keys
            .insert("v2".to_string(), hex::decode(key2).unwrap());

        let now = 1700000000;
        let exp = now + 3600;

        let params = vec![
            (
                "src".to_string(),
                "https://images.example.com/cat.jpg".to_string(),
            ),
            ("kid".to_string(), "v2".to_string()),
            ("exp".to_string(), exp.to_string()),
        ];
        let canonical = build_canonical_string(exp, &params);
        let sig = compute_signature(&hex::decode(key2).unwrap(), &canonical);

        let query =
            format!("src=https%3A%2F%2Fimages.example.com%2Fcat.jpg&kid=v2&exp={exp}&sig={sig}");
        let res = verify_signature(&config, &query, now);
        assert_eq!(res.unwrap(), Some("v2".to_string()));
    }
}
