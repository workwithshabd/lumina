#![no_main]

use libfuzzer_sys::fuzz_target;
use atihsrah::auth::build_canonical_string;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    let mut exp_bytes = [0u8; 8];
    exp_bytes.copy_from_slice(&data[0..8]);
    let exp = u64::from_le_bytes(exp_bytes);

    let remainder = &data[8..];
    if let Ok(text) = std::str::from_utf8(remainder) {
        let mut params = Vec::new();
        for chunk in text.split('&') {
            if let Some((k, v)) = chunk.split_once('=') {
                params.push((k.to_string(), v.to_string()));
            } else if !chunk.is_empty() {
                params.push((chunk.to_string(), String::new()));
            }
        }

        // Must never panic on arbitrary parameter keys/values
        let canon1 = build_canonical_string(exp, &params);
        let canon2 = build_canonical_string(exp, &params);

        // Canonical representation must be strictly deterministic
        assert_eq!(canon1, canon2, "build_canonical_string must be deterministic");

        // Canonical string must start with standard prefix
        assert!(
            canon1.starts_with("GET\n/transform\n"),
            "Canonical string must start with GET\\n/transform\\n"
        );
    }
});

