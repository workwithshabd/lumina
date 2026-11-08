pub mod params;
pub mod routes;
pub mod transform;

use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tower::limit::ConcurrencyLimitLayer;

use crate::error::CURRENT_REQUEST_ID;
use crate::state::AppState;

pub static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
pub static X_CONTENT_TYPE_OPTIONS: HeaderName = HeaderName::from_static("x-content-type-options");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(pub String);

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Middleware that sets security headers on all HTTP responses.
pub async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    resp.headers_mut().insert(
        X_CONTENT_TYPE_OPTIONS.clone(),
        HeaderValue::from_static("nosniff"),
    );
    resp
}

/// Middleware that tracks inflight HTTP requests count.
pub async fn inflight_tracking_middleware(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    state.on_request_start();
    struct InflightGuard(Arc<AppState>);
    impl Drop for InflightGuard {
        fn drop(&mut self) {
            self.0.on_request_end();
        }
    }
    let _guard = InflightGuard(state);
    next.run(req).await
}

/// Middleware that validates header limits (count and total size).
pub async fn header_limits_middleware(
    req: Request,
    next: Next,
) -> Result<Response, crate::error::AppError> {
    if req.headers().len() > 64 {
        return Err(crate::error::AppError::HeadersTooLarge(
            "Too many HTTP headers in request (max 64)".to_string(),
        ));
    }
    let total_len: usize = req
        .headers()
        .iter()
        .map(|(k, v)| k.as_str().len() + v.as_bytes().len())
        .sum();
    if total_len > 32768 {
        return Err(crate::error::AppError::HeadersTooLarge(
            "HTTP request headers exceed maximum size of 32KB".to_string(),
        ));
    }
    Ok(next.run(req).await)
}

/// Middleware that validates request body limits against configured maximum.
pub async fn body_limit_middleware(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, crate::error::AppError> {
    if let Some(cl) = req.headers().get(axum::http::header::CONTENT_LENGTH) {
        if let Ok(cl_str) = cl.to_str() {
            if let Ok(len) = cl_str.parse::<usize>() {
                if len > state.config.max_body_bytes {
                    return Err(crate::error::AppError::PayloadTooLarge(format!(
                        "Request content length of {} bytes exceeds maximum allowed limit of {} bytes",
                        len, state.config.max_body_bytes
                    )));
                }
            }
        }
    }
    Ok(next.run(req).await)
}

/// Middleware that enforces a maximum request timeout.
pub async fn request_timeout_middleware(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, crate::error::AppError> {
    let timeout_dur = Duration::from_secs(state.config.request_timeout_secs);
    tokio::time::timeout(timeout_dur, next.run(req))
        .await
        .map_err(|_| crate::error::AppError::FetchTimeout)
}

/// Middleware that assigns/propagates a ULID request ID, registers it in
/// extensions and task-local storage, and sets the X-Request-ID response header.
pub async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    let req_id = match req.headers().get(&X_REQUEST_ID) {
        Some(val) => match val.to_str() {
            Ok(s) => {
                let trimmed = s.trim();
                // Validate reasonable length and characters for an incoming request ID
                if !trimmed.is_empty()
                    && trimmed.len() <= 64
                    && trimmed
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    trimmed.to_string()
                } else {
                    ulid::Ulid::new().to_string()
                }
            }
            Err(_) => ulid::Ulid::new().to_string(),
        },
        None => ulid::Ulid::new().to_string(),
    };

    req.extensions_mut().insert(RequestId(req_id.clone()));
    if let Ok(hdr_val) = HeaderValue::from_str(&req_id) {
        req.headers_mut().insert(X_REQUEST_ID.clone(), hdr_val);
    }

    let mut response = CURRENT_REQUEST_ID
        .scope(req_id.clone(), next.run(req))
        .await;

    if let Ok(hdr_val) = HeaderValue::from_str(&req_id) {
        response.headers_mut().insert(X_REQUEST_ID.clone(), hdr_val);
    }

    response
}

/// Builds the Axum router for Atihsrah with full hardening and observability layers.
pub fn create_router(state: Arc<AppState>) -> Router {
    let max_concurrency = state.config.global_concurrency_limit;
    let max_body_bytes = state.config.max_body_bytes;

    Router::new()
        .route("/health", get(routes::health_handler))
        .route("/ready", get(routes::ready_handler))
        .route("/transform", get(transform::transform_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            inflight_tracking_middleware,
        ))
        .layer(axum::middleware::from_fn(security_headers_middleware))
        .layer(axum::middleware::from_fn(header_limits_middleware))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            body_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            request_timeout_middleware,
        ))
        .layer(axum::middleware::from_fn(request_id_middleware))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(ConcurrencyLimitLayer::new(max_concurrency))
        .with_state(state)
}
