#![no_main]

use libfuzzer_sys::fuzz_target;
use atihsrah::http::params::TransformParams;

fuzz_target!(|data: &[u8]| {
    if let Ok(query_str) = std::str::from_utf8(data) {
        // Must never panic on arbitrary UTF-8 query strings
        let _ = TransformParams::parse(Some(query_str));
    }
});

