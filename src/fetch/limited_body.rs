use reqwest::header::CONTENT_TYPE;
use reqwest::Response;

use crate::error::AppError;

/// Reads an HTTP response body with strict byte limiting and Content-Type verification.
///
/// Security properties:
/// 1. Verifies HTTP status is successful (and explicitly rejects 3xx redirects).
/// 2. Requires `Content-Type` starting with `image/` (rejects html/json/scripts/executables).
/// 3. Checks `Content-Length` header upfront if present; aborts if it exceeds `max_source_fetch_mb`.
/// 4. Streams response chunks incrementally and aborts immediately as soon as cumulative bytes
///    exceed `max_source_fetch_mb`, regardless of whether `Content-Length` was omitted,
///    spoofed, or chunked-transfer-encoded.
pub async fn read_limited_body(
    mut response: Response,
    max_source_fetch_mb: usize,
) -> Result<Vec<u8>, AppError> {
    let status = response.status();

    // 1. Status code validation: explicit rejection of redirects and origin failures
    if status.is_redirection() {
        return Err(AppError::FetchFailed(format!(
            "Redirects are prohibited: origin returned HTTP {status}"
        )));
    }

    if !status.is_success() {
        return Err(AppError::FetchFailed(format!(
            "Origin returned unsuccessful HTTP status: {status}"
        )));
    }

    // 2. Content-Type requirement: must start with image/
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();

    if !content_type.starts_with("image/") {
        return Err(AppError::NotAnImage(format!(
            "Origin response Content-Type must start with 'image/', found '{content_type}'"
        )));
    }

    let max_bytes = max_source_fetch_mb * 1024 * 1024;

    // 3. Early check on Content-Length header if declared
    if let Some(content_length) = response.content_length() {
        if content_length as usize > max_bytes {
            return Err(AppError::SourceTooLarge(content_length as usize));
        }
    }

    // 4. Stream body chunks with strict cumulative size boundary
    let mut buffer = Vec::new();

    while let Some(chunk) = response.chunk().await.map_err(|e| {
        if e.is_timeout() {
            AppError::FetchTimeout
        } else {
            AppError::FetchFailed(format!("Error reading response stream: {e}"))
        }
    })? {
        if buffer.len() + chunk.len() > max_bytes {
            return Err(AppError::SourceTooLarge(buffer.len() + chunk.len()));
        }
        buffer.extend_from_slice(&chunk);
    }

    if buffer.is_empty() {
        return Err(AppError::NotAnImage(
            "Origin returned empty response body".to_string(),
        ));
    }

    Ok(buffer)
}
