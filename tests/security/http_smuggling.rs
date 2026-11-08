use atihsrah::config::AuthMode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::server::TestServer;

#[tokio::test]
async fn test_conflicting_content_length_rejected() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    // Send raw HTTP request with conflicting Content-Length headers
    let mut stream = TcpStream::connect(server.addr).await.unwrap();
    let raw_request =
        "GET /transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n\
         Content-Length: 5\r\n\
         Content-Length: 10\r\n\
         Connection: close\r\n\r\n\
         12345";

    stream.write_all(raw_request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let resp_str = String::from_utf8_lossy(&response);

    // Axum/Hyper or our middleware must reject conflicting Content-Length with 400 Bad Request
    assert!(
        resp_str.contains("400 Bad Request") || resp_str.contains("431") || resp_str.is_empty(),
        "Conflicting Content-Length should be rejected with 400 Bad Request, got:\n{resp_str}"
    );
}

#[tokio::test]
async fn test_oversized_headers_rejected_with_431() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    // 1. Total header size > 32KB
    let mut stream = TcpStream::connect(server.addr).await.unwrap();
    let large_value = "A".repeat(35000);
    let raw_request = format!(
        "GET /health HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n\
         X-Huge-Header: {}\r\n\
         Connection: close\r\n\r\n",
        large_value
    );

    stream.write_all(raw_request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let resp_str = String::from_utf8_lossy(&response);

    assert!(
        resp_str.contains("431 Request Header Fields Too Large")
            || resp_str.contains("HEADERS_TOO_LARGE"),
        "Oversized header must be rejected with 431, got:\n{resp_str}"
    );

    // 2. More than 64 individual headers
    let mut stream2 = TcpStream::connect(server.addr).await.unwrap();
    let mut multi_headers = String::from("GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n");
    for i in 0..70 {
        multi_headers.push_str(&format!("X-Header-{i}: val{i}\r\n"));
    }
    multi_headers.push_str("Connection: close\r\n\r\n");

    stream2.write_all(multi_headers.as_bytes()).await.unwrap();
    let mut response2 = Vec::new();
    stream2.read_to_end(&mut response2).await.unwrap();
    let resp_str2 = String::from_utf8_lossy(&response2);

    assert!(
        resp_str2.contains("431 Request Header Fields Too Large")
            || resp_str2.contains("HEADERS_TOO_LARGE"),
        "Header count exceeding 64 must be rejected with 431, got:\n{resp_str2}"
    );
}

#[tokio::test]
async fn test_invalid_utf8_in_query_rejected() {
    let server =
        TestServer::start(AuthMode::Public, vec!["https://images.example.com"], None).await;

    let mut stream = TcpStream::connect(server.addr).await.unwrap();
    // Non-UTF8 byte sequence in percent-encoded query: %FF%FE%FD
    let raw_request =
        "GET /transform?src=https%3A%2F%2Fimages.example.com%2Fphoto.jpg&w=%FF%FE%FD HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n\
         Connection: close\r\n\r\n";

    stream.write_all(raw_request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let resp_str = String::from_utf8_lossy(&response);

    // Must be rejected with 400 Bad Request or 422 Unprocessable Entity, never internal crash 500
    assert!(
        resp_str.contains("400 Bad Request") || resp_str.contains("422 Unprocessable Entity"),
        "Invalid UTF-8 in query parameters must be rejected gracefully, got:\n{resp_str}"
    );
}
