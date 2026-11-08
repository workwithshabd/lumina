use atihsrah::auth::signing::{build_canonical_string, compute_signature, verify_signature};
use atihsrah::config::Config;
use atihsrah::error::AppError;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Deserialize)]
struct VectorFile {
    vectors: Vec<TestVector>,
}

#[derive(Debug, Deserialize)]
struct TestVector {
    id: String,
    description: String,
    #[serde(rename = "type")]
    vector_type: String,
    key: String,
    kid: Option<String>,
    params: HashMap<String, String>,
    exp: u64,
    now_secs: u64,
    canonical_string: String,
    expected_sig: String,
    raw_query: String,
    expected_error: Option<String>,
}

fn build_test_config(key_hex: &str, kid: Option<&str>) -> Config {
    let mut map = HashMap::new();
    map.insert(
        "ALLOWED_ORIGINS".to_string(),
        "https://images.example.com".to_string(),
    );
    map.insert("ATIHSRAH_AUTH_MODE".to_string(), "signed".to_string());
    map.insert(
        "ATIHSRAH_SIGNING_KEY".to_string(),
        "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_string(),
    );

    let mut config = Config::from_lookup(|k| map.get(k).cloned()).expect("valid config");
    let key_name = kid.unwrap_or("default");
    let key_bytes = hex::decode(key_hex).expect("valid hex key");
    config.signing_keys.clear();
    config
        .signing_keys
        .insert(key_name.to_string(), key_bytes.clone());
    config.signing_key = Some(key_bytes);
    config
}

#[test]
fn test_all_signing_vectors() {
    let content = fs::read_to_string("docs/signing-vectors.json")
        .expect("docs/signing-vectors.json must be readable");
    let parsed: VectorFile =
        serde_json::from_str(&content).expect("docs/signing-vectors.json must be valid JSON");

    assert!(
        parsed.vectors.len() >= 15,
        "Specification requires at least 15 vectors, got {}",
        parsed.vectors.len()
    );

    for vec in &parsed.vectors {
        println!("Testing vector: {} - {}", vec.id, vec.description);

        if vec.vector_type == "positive" {
            // 1. Verify canonical string computation
            let mut params_vec: Vec<(String, String)> = vec
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if let Some(ref kid) = vec.kid {
                params_vec.push(("kid".to_string(), kid.clone()));
            }
            let canonical = build_canonical_string(vec.exp, &params_vec);
            assert_eq!(
                canonical, vec.canonical_string,
                "Canonical string mismatch for {}",
                vec.id
            );

            // 2. Verify signature computation
            let key_bytes = hex::decode(&vec.key).expect("valid hex key");
            let sig = compute_signature(&key_bytes, &canonical);
            assert_eq!(sig, vec.expected_sig, "Signature mismatch for {}", vec.id);

            // 3. Verify server verify_signature accepts the request
            let config = build_test_config(&vec.key, vec.kid.as_deref());
            let verified = verify_signature(&config, &vec.raw_query, vec.now_secs);
            assert!(
                verified.is_ok(),
                "Positive vector {} failed verification: {:?}",
                vec.id,
                verified.err()
            );

            if let Some(ref expected_kid) = vec.kid {
                assert_eq!(
                    verified.unwrap(),
                    Some(expected_kid.clone()),
                    "KID mismatch for {}",
                    vec.id
                );
            }
        } else if vec.vector_type == "negative" {
            // Negative vector: must be rejected
            let config = build_test_config(&vec.key, vec.kid.as_deref());
            let verified = verify_signature(&config, &vec.raw_query, vec.now_secs);

            assert!(
                verified.is_err(),
                "Negative vector {} was unexpectedly accepted: {:?}",
                vec.id,
                verified
            );

            let err = verified.unwrap_err();
            let expected_kind = vec
                .expected_error
                .as_deref()
                .expect("negative vector must specify expected_error");

            match expected_kind {
                "DuplicateParam" => {
                    assert!(
                        matches!(err, AppError::DuplicateParam(_)),
                        "Vector {} expected DuplicateParam, got {:?}",
                        vec.id,
                        err
                    );
                }
                "Expired" => {
                    assert!(
                        matches!(err, AppError::Expired(_)),
                        "Vector {} expected Expired, got {:?}",
                        vec.id,
                        err
                    );
                }
                "Unauthorized" => {
                    assert!(
                        matches!(err, AppError::Unauthorized(_)),
                        "Vector {} expected Unauthorized, got {:?}",
                        vec.id,
                        err
                    );
                }
                other => panic!("Unknown expected error: {other}"),
            }
        } else {
            panic!("Unknown vector type: {}", vec.vector_type);
        }
    }
}
