use atihsrah::config::AuthMode;
use reqwest::StatusCode;

use super::server::{response_json, TestServer};

#[tokio::test]
async fn test_parameter_pollution_duplicate_and_unknown_rejected() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    let client = reqwest::Client::new();

    // 1. Duplicate parameter pollution: ?w=100&w=200
    let dup_uri = format!(
        "http://{}/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=100&w=200",
        server.addr
    );
    let resp = client.get(&dup_uri).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let json = response_json(resp).await;
    assert_eq!(json["code"], "DUPLICATE_PARAM");

    // 2. Unknown parameter injection: ?admin=true
    let unknown_uri = format!(
        "http://{}/transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&admin=true",
        server.addr
    );
    let resp2 = client.get(&unknown_uri).send().await.unwrap();
    assert_eq!(resp2.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let json2 = response_json(resp2).await;
    assert_eq!(json2["code"], "UNKNOWN_PARAM");
}

#[tokio::test]
async fn test_signature_bypass_parameter_tampering_and_case() {
    let server =
        TestServer::start(AuthMode::Signed, vec!["https://images.example.com"], None).await;

    let client = reqwest::Client::new();
    let exp = server.now_exp(3600);

    let src = "https://images.example.com/secure.jpg";
    let params = vec![
        ("exp".to_string(), exp.to_string()),
        ("format".to_string(), "webp".to_string()),
        ("src".to_string(), src.to_string()),
        ("w".to_string(), "100".to_string()),
    ];

    let query_signed = server.sign_params(exp, &params);

    // 1. Valid signed request succeeds
    let valid_uri = format!("http://{}/transform?{}", server.addr, query_signed);
    let resp = client.get(&valid_uri).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Tampered parameter value: change w=100 to w=200 with same signature
    let tampered_uri = format!(
        "http://{}/transform?{}",
        server.addr,
        query_signed.replace("w=100", "w=200")
    );
    let resp2 = client.get(&tampered_uri).send().await.unwrap();
    assert_eq!(resp2.status(), StatusCode::UNAUTHORIZED);
    let json2 = response_json(resp2).await;
    assert_eq!(json2["code"], "UNAUTHORIZED");

    // 3. Duplicate parameter injection bypass: append another param at the end
    let injected_uri = format!("{valid_uri}&w=500");
    let resp3 = client.get(&injected_uri).send().await.unwrap();
    assert_eq!(resp3.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let json3 = response_json(resp3).await;
    assert_eq!(json3["code"], "DUPLICATE_PARAM");

    // 4. Parameter case variation: uppercase keys (e.g. W=100, FORMAT=webp)
    // Keys are strictly lowercase; uppercase keys must be rejected as unknown params
    let upper_uri = format!(
        "http://{}/transform?src=https%3A%2F%2Fimages.example.com%2Fsecure.jpg&W=100",
        server.addr
    );
    let resp4 = client.get(&upper_uri).send().await.unwrap();
    assert_eq!(resp4.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let json4 = response_json(resp4).await;
    assert_eq!(json4["code"], "UNKNOWN_PARAM");

    // 5. Signature 1-bit tampering: replace last character of signature
    let mut bad_sig_query = query_signed.clone();
    let last_char = bad_sig_query.pop().unwrap();
    let replacement_char = if last_char == '0' { '1' } else { '0' };
    bad_sig_query.push(replacement_char);

    let bad_sig_uri = format!("http://{}/transform?{}", server.addr, bad_sig_query);
    let resp5 = client.get(&bad_sig_uri).send().await.unwrap();
    assert_eq!(resp5.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_signature_unicode_normalization_bypass_attempt() {
    let server =
        TestServer::start(AuthMode::Signed, vec!["https://images.example.com"], None).await;

    let client = reqwest::Client::new();
    let exp = server.now_exp(3600);

    // Test Unicode normalization attack:
    // "café" in NFC is \u{00E9} (single code point: c3 a9)
    // "café" in NFD is e + \u{0301} (decomposed: 65 cc 81)
    let nfc_url = "https://images.example.com/caf\u{00E9}.jpg";
    let nfd_url = "https://images.example.com/cafe\u{0301}.jpg";

    let params_nfc = vec![
        ("exp".to_string(), exp.to_string()),
        ("src".to_string(), nfc_url.to_string()),
    ];
    let query_nfc = server.sign_params(exp, &params_nfc);

    // Valid NFC request succeeds
    let valid_nfc_uri = format!("http://{}/transform?{}", server.addr, query_nfc);
    let resp_nfc = client.get(&valid_nfc_uri).send().await.unwrap();
    assert_eq!(resp_nfc.status(), StatusCode::OK);

    // Attacker sends NFD decomposed URL trying to use NFC signature -> must fail
    let nfc_sig = query_nfc.split("sig=").nth(1).unwrap();
    let nfd_tampered_query = format!(
        "exp={}&src={}&sig={}",
        exp,
        percent_encoding::utf8_percent_encode(nfd_url, percent_encoding::NON_ALPHANUMERIC),
        nfc_sig
    );
    let nfd_uri = format!("http://{}/transform?{}", server.addr, nfd_tampered_query);
    let resp_nfd = client.get(&nfd_uri).send().await.unwrap();
    assert_eq!(
        resp_nfd.status(),
        StatusCode::UNAUTHORIZED,
        "Unicode NFD variation with NFC signature must be rejected"
    );
}
