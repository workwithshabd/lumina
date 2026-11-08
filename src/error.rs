use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use thiserror::Error;

tokio::task_local! {
    pub static CURRENT_REQUEST_ID: String;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
    pub request_id: String,
    pub hint: String,
}

#[derive(Debug, Clone, Error)]
pub enum AppError {
    #[error("Invalid parameter: {0}")]
    InvalidParam(String),

    #[error("Duplicate parameter: {0}")]
    DuplicateParam(String),

    #[error("Unknown parameter: {0}")]
    UnknownParam(String),

    #[error("Source origin not allowed: {0}")]
    SourceNotAllowed(String),

    #[error("SSRF protection blocked request: {0}")]
    SsrfBlocked(String),

    #[error("Source image fetch timed out")]
    FetchTimeout,

    #[error("Failed to fetch source image: {0}")]
    FetchFailed(String),

    #[error("Source image exceeds maximum allowed size ({0} bytes)")]
    SourceTooLarge(usize),

    #[error("Source is not a supported image: {0}")]
    NotAnImage(String),

    #[error("Image dimensions exceed maximum limits: {0}")]
    ImageTooLarge(String),

    #[error("Failed to decode image: {0}")]
    DecodeFailed(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Request signature has expired: {0}")]
    Expired(String),

    #[error("Rate limit exceeded. Please retry later.")]
    RateLimited { retry_after_secs: u64 },

    #[error("Request payload exceeds limit: {0}")]
    PayloadTooLarge(String),

    #[error("Request headers exceed limit: {0}")]
    HeadersTooLarge(String),

    #[error("Internal server error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidParam(_) => StatusCode::BAD_REQUEST,
            Self::DuplicateParam(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::UnknownParam(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::SourceNotAllowed(_) => StatusCode::FORBIDDEN,
            Self::SsrfBlocked(_) => StatusCode::FORBIDDEN,
            Self::FetchTimeout => StatusCode::GATEWAY_TIMEOUT,
            Self::FetchFailed(_) => StatusCode::BAD_GATEWAY,
            Self::SourceTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::NotAnImage(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::ImageTooLarge(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::DecodeFailed(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Expired(_) => StatusCode::GONE,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::HeadersTooLarge(_) => StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidParam(_) => "INVALID_PARAM",
            Self::DuplicateParam(_) => "DUPLICATE_PARAM",
            Self::UnknownParam(_) => "UNKNOWN_PARAM",
            Self::SourceNotAllowed(_) => "SOURCE_NOT_ALLOWED",
            Self::SsrfBlocked(_) => "SSRF_BLOCKED",
            Self::FetchTimeout => "FETCH_TIMEOUT",
            Self::FetchFailed(_) => "FETCH_FAILED",
            Self::SourceTooLarge(_) => "SOURCE_TOO_LARGE",
            Self::NotAnImage(_) => "NOT_AN_IMAGE",
            Self::ImageTooLarge(_) => "IMAGE_TOO_LARGE",
            Self::DecodeFailed(_) => "DECODE_FAILED",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::Expired(_) => "EXPIRED",
            Self::RateLimited { .. } => "RATE_LIMITED",
            Self::PayloadTooLarge(_) => "PAYLOAD_TOO_LARGE",
            Self::HeadersTooLarge(_) => "HEADERS_TOO_LARGE",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            Self::InvalidParam(_) => "Verify that all query parameters match the expected format and bounds.",
            Self::DuplicateParam(_) => "Remove duplicated query parameters from the request.",
            Self::UnknownParam(_) => "Only supported parameters (e.g. w, h, fit, f, q, s) are permitted.",
            Self::SourceNotAllowed(_) => "Ensure the image URL origin is listed in the server's ALLOWED_ORIGINS.",
            Self::SsrfBlocked(_) => "Requests to private, loopback, link-local, or reserved networks are blocked.",
            Self::FetchTimeout => "The origin server took too long to respond. Check origin latency.",
            Self::FetchFailed(_) => "The origin server could not be reached or returned an error.",
            Self::SourceTooLarge(_) => "The image file exceeds the MAX_SOURCE_FETCH_MB limit.",
            Self::NotAnImage(_) => "The fetched resource did not match any supported image format headers or magic bytes.",
            Self::ImageTooLarge(_) => "Image width or height exceeds MAX_SOURCE_DIMENSION or MAX_OUTPUT_DIMENSION.",
            Self::DecodeFailed(_) => "The image data appears corrupted or unparseable by the engine.",
            Self::Unauthorized(_) => "Provide a valid HMAC signature matching the request URL and signing key.",
            Self::Expired(_) => "The expiration timestamp (exp) in the signature has passed.",
            Self::RateLimited { .. } => "You have exceeded the allowed request rate. Back off before retrying.",
            Self::PayloadTooLarge(_) => "Ensure the request body size does not exceed the server limit.",
            Self::HeadersTooLarge(_) => "Reduce the number or size of HTTP request headers.",
            Self::Internal(_) => "An internal processing error occurred. Check server logs.",
        }
    }

    pub fn to_response_body(&self, request_id: &str) -> ErrorResponse {
        ErrorResponse {
            error: self.to_string(),
            code: self.code().to_string(),
            request_id: request_id.to_string(),
            hint: self.hint().to_string(),
        }
    }

    pub fn to_response_with_id(&self, request_id: &str) -> Response {
        let status = self.status_code();
        let body = self.to_response_body(request_id);
        let mut resp = (status, Json(body)).into_response();
        if let Ok(hdr) = HeaderValue::from_str(request_id) {
            resp.headers_mut()
                .insert(HeaderName::from_static("x-request-id"), hdr);
        }
        if let AppError::RateLimited { retry_after_secs } = self {
            let wait_secs = (*retry_after_secs).max(1);
            if let Ok(val) = HeaderValue::from_str(&wait_secs.to_string()) {
                resp.headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, val);
            }
        }
        resp
    }
}

impl From<crate::processor::ProcessError> for AppError {
    fn from(err: crate::processor::ProcessError) -> Self {
        match err {
            crate::processor::ProcessError::UnsupportedFormat(msg) => AppError::NotAnImage(msg),
            crate::processor::ProcessError::DecodeFailed(msg) => AppError::DecodeFailed(msg),
            crate::processor::ProcessError::LimitsExceeded(msg) => AppError::ImageTooLarge(msg),
            crate::processor::ProcessError::ProcessingFailed(msg) => AppError::Internal(msg),
            crate::processor::ProcessError::Internal(msg) => AppError::Internal(msg),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let request_id = CURRENT_REQUEST_ID
            .try_with(|id| id.clone())
            .unwrap_or_else(|_| ulid::Ulid::new().to_string());
        self.to_response_with_id(&request_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_status_and_json_mapping() {
        let cases: Vec<(AppError, StatusCode, &'static str)> = vec![
            (
                AppError::InvalidParam("w must be integer".into()),
                StatusCode::BAD_REQUEST,
                "INVALID_PARAM",
            ),
            (
                AppError::DuplicateParam("w".into()),
                StatusCode::UNPROCESSABLE_ENTITY,
                "DUPLICATE_PARAM",
            ),
            (
                AppError::UnknownParam("foo".into()),
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNKNOWN_PARAM",
            ),
            (
                AppError::SourceNotAllowed("https://evil.com".into()),
                StatusCode::FORBIDDEN,
                "SOURCE_NOT_ALLOWED",
            ),
            (
                AppError::SsrfBlocked("127.0.0.1".into()),
                StatusCode::FORBIDDEN,
                "SSRF_BLOCKED",
            ),
            (
                AppError::FetchTimeout,
                StatusCode::GATEWAY_TIMEOUT,
                "FETCH_TIMEOUT",
            ),
            (
                AppError::FetchFailed("Connection refused".into()),
                StatusCode::BAD_GATEWAY,
                "FETCH_FAILED",
            ),
            (
                AppError::SourceTooLarge(50_000_000),
                StatusCode::PAYLOAD_TOO_LARGE,
                "SOURCE_TOO_LARGE",
            ),
            (
                AppError::NotAnImage("text/html".into()),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "NOT_AN_IMAGE",
            ),
            (
                AppError::ImageTooLarge("10000x10000".into()),
                StatusCode::UNPROCESSABLE_ENTITY,
                "IMAGE_TOO_LARGE",
            ),
            (
                AppError::DecodeFailed("Corrupted PNG header".into()),
                StatusCode::UNPROCESSABLE_ENTITY,
                "DECODE_FAILED",
            ),
            (
                AppError::Unauthorized("Invalid HMAC signature".into()),
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
            ),
            (
                AppError::Expired("Timestamp 1600000000".into()),
                StatusCode::GONE,
                "EXPIRED",
            ),
            (
                AppError::RateLimited {
                    retry_after_secs: 60,
                },
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
            ),
            (
                AppError::PayloadTooLarge("exceeds 64KB".into()),
                StatusCode::PAYLOAD_TOO_LARGE,
                "PAYLOAD_TOO_LARGE",
            ),
            (
                AppError::HeadersTooLarge("headers exceed 32KB".into()),
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                "HEADERS_TOO_LARGE",
            ),
            (
                AppError::Internal("Worker thread pool panicked".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
            ),
        ];

        let test_req_id = "01JTESTREQUESTID00000000000";

        for (err, expected_status, expected_code) in cases {
            assert_eq!(err.status_code(), expected_status);
            assert_eq!(err.code(), expected_code);

            let body = err.to_response_body(test_req_id);
            assert_eq!(body.code, expected_code);
            assert_eq!(body.request_id, test_req_id);
            assert!(!body.error.is_empty());
            assert!(!body.hint.is_empty());

            let json_str = serde_json::to_string(&body).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

            assert_eq!(parsed["code"], expected_code);
            assert_eq!(parsed["request_id"], test_req_id);
            assert!(parsed.get("error").is_some());
            assert!(parsed.get("hint").is_some());
        }
    }

    #[test]
    fn test_into_response_sets_header() {
        let err = AppError::InvalidParam("bad width".into());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(resp.headers().contains_key("x-request-id"));

        let rate_err = AppError::RateLimited {
            retry_after_secs: 42,
        };
        let rate_resp = rate_err.into_response();
        assert_eq!(rate_resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(rate_resp.headers().get("retry-after").unwrap(), "42");
    }
}
