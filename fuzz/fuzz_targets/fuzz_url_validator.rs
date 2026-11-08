#![no_main]

use libfuzzer_sys::fuzz_target;
use std::collections::HashSet;
use atihsrah::fetch::validator::validate_source_url;

fuzz_target!(|data: &[u8]| {
    if let Ok(url_str) = std::str::from_utf8(data) {
        let mut allowed = HashSet::new();
        allowed.insert("https://images.example.com".to_string());
        allowed.insert("https://cdn.example.org:8443".to_string());
        allowed.insert("https://allowed.domain.co.uk".to_string());

        // Must never panic on arbitrary URL strings
        if let Ok(parsed) = validate_source_url(url_str, &allowed) {
            // Post-condition invariants:
            assert_eq!(parsed.scheme(), "https", "Parsed URL must have https scheme");
            assert!(parsed.username().is_empty(), "Parsed URL must not have username");
            assert!(parsed.password().is_none(), "Parsed URL must not have password");
            assert!(parsed.fragment().is_none(), "Parsed URL must not have fragment");
            assert!(
                allowed.iter().any(|origin| origin.starts_with(&parsed.origin().ascii_serialization())),
                "Parsed URL origin must match an allowed origin"
            );
        }
    }
});

