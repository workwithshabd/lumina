use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{HeaderName, HeaderValue, ACCEPT, CACHE_CONTROL, CONTENT_TYPE, VARY};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::auth::{extract_client_ip, resolve_rate_limit_key, verify_signature};
use crate::cache::inflight::{InflightError, InflightRole};
use crate::cache::{compute_cache_key, CacheKeyParams, CacheStatus, CachedImage};
use crate::config::AuthMode;
use crate::error::AppError;
use crate::http::params::{
    is_format_acceptable, resolve_cache_format, resolve_format, resolve_quality, AcceptClass,
    CacheFormat, FormatParam, TransformParams,
};
use crate::processor::{ImageFormat, ProcessOptions};
use crate::state::AppState;

#[derive(Clone, Copy, Debug, Default)]
pub struct ResponseTimings {
    pub fetch_ms: Option<u128>,
    pub process_ms: Option<u128>,
    pub total_ms: u128,
}

pub static X_ATIHSRAH_CACHE: HeaderName = HeaderName::from_static("x-atihsrah-cache");
pub static X_DEBUG_FETCH_TIME: HeaderName = HeaderName::from_static("x-atihsrah-fetch-ms");
pub static X_DEBUG_PROCESS_TIME: HeaderName = HeaderName::from_static("x-atihsrah-process-ms");
pub static X_DEBUG_TOTAL_TIME: HeaderName = HeaderName::from_static("x-atihsrah-total-ms");

/// GET /transform
///
/// Complete image transformation pipeline:
/// 1. Parse & strictly validate query parameters.
/// 2. Predict output format and check in-memory cache (Moka).
/// 3. On miss, coordinate single-flight execution (InflightManager) so concurrent
///    identical requests perform only ONE origin fetch and image processing task.
/// 4. Fetch source image via SourceFetcher (Phase 3 SSRF & DNS-pinned).
/// 5. Probe header-only: validate dimensions and verify estimated processing memory
///    (w * h * channels * 3) <= MAX_PROCESSING_MEMORY_MB to reject decompression bombs.
/// 6. Execute transformation inside `spawn_blocking` capped by concurrency semaphore.
/// 7. Clamp dimensions to MAX_OUTPUT_DIMENSION and prevent upscaling unless explicitly permitted.
/// 8. If output <= CACHE_SINGLE_ENTRY_MAX_MB, insert into Moka cache; otherwise skip caching
///    and increment oversize metric.
/// 9. Emit HTTP response with headers: Content-Type, Cache-Control, Vary, X-Atihsrah-Cache,
///    and measured debug timing headers if enabled.
pub async fn transform_handler(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Response, AppError> {
    let start_time = Instant::now();

    let headers = req.headers().clone();
    let peer_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0)
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 0)));

    let request_id = req
        .extensions()
        .get::<crate::http::RequestId>()
        .map(|r| r.0.clone())
        .or_else(|| {
            crate::error::CURRENT_REQUEST_ID
                .try_with(|id| id.clone())
                .ok()
        })
        .or_else(|| {
            headers
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "-".to_string());

    let forwarded_for = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    let client_ip = extract_client_ip(peer_addr, forwarded_for, &state.config.trusted_proxy_cidrs);

    let res = handle_transform(&state, req, client_ip, start_time).await;

    let duration_secs = start_time.elapsed().as_secs_f64();
    let duration_ms = start_time.elapsed().as_millis();

    match res {
        Ok((resp, cache_result, src_sha)) => {
            crate::observability::record_request(cache_result, 200, duration_secs);
            crate::observability::log_access(
                &request_id,
                "GET",
                "/transform",
                200,
                duration_ms,
                cache_result,
                src_sha.as_deref(),
                client_ip,
            );
            Ok(resp)
        }
        Err((err, src_sha)) => {
            let status = err.status_code().as_u16();
            crate::observability::record_request("none", status, duration_secs);

            match &err {
                AppError::SsrfBlocked(reason) => {
                    crate::observability::record_ssrf_blocked();
                    crate::observability::log_security_event(
                        &request_id,
                        "ssrf_blocked",
                        reason,
                        src_sha.as_deref(),
                        Some(client_ip),
                    );
                }
                AppError::RateLimited { .. } => {
                    crate::observability::record_rate_limited();
                    crate::observability::log_security_event(
                        &request_id,
                        "rate_limited",
                        "Rate limit exceeded",
                        src_sha.as_deref(),
                        Some(client_ip),
                    );
                }
                AppError::Unauthorized(reason) | AppError::Expired(reason) => {
                    crate::observability::record_signature_invalid();
                    crate::observability::log_security_event(
                        &request_id,
                        "auth_failure",
                        reason,
                        src_sha.as_deref(),
                        Some(client_ip),
                    );
                }
                AppError::SourceNotAllowed(reason) => {
                    crate::observability::record_source_rejected("disallowed_origin");
                    crate::observability::log_security_event(
                        &request_id,
                        "source_rejected",
                        reason,
                        src_sha.as_deref(),
                        Some(client_ip),
                    );
                }
                _ => {
                    crate::observability::record_error(err.code());
                    crate::observability::log_error(&request_id, err.code(), &err.to_string());
                }
            }

            crate::observability::log_access(
                &request_id,
                "GET",
                "/transform",
                status,
                duration_ms,
                "none",
                src_sha.as_deref(),
                client_ip,
            );

            Err(err)
        }
    }
}

async fn handle_transform(
    state: &Arc<AppState>,
    req: Request,
    client_ip: std::net::IpAddr,
    start_time: Instant,
) -> Result<(Response, &'static str, Option<String>), (AppError, Option<String>)> {
    let headers = req.headers();
    let uri = req.uri();

    // 1. Parse and validate query parameters (rejects duplicates & unknowns with 422)
    let raw_query = uri.query().unwrap_or("");
    let params = match TransformParams::parse(Some(raw_query)) {
        Ok(p) => p,
        Err(e) => return Err((e, None)),
    };

    let src_sha = Some(crate::observability::sha256_prefix(&params.src));

    // 2. Pre-auth rate limiting on client IP (generous quota to prevent DDoS before signature check)
    let ip_key = format!("ip:{client_ip}");
    if let Err(e) = state.rate_limiter.check_pre_auth(&ip_key) {
        return Err((e, src_sha));
    }

    // 3. Authenticate Request Signature (if AUTH_MODE=signed) BEFORE any key-based limiting
    let rate_key = match state.config.auth_mode {
        AuthMode::Signed => {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let verified_kid = verify_signature(&state.config, raw_query, now_secs)
                .map_err(|e| (e, src_sha.clone()))?;
            resolve_rate_limit_key(&AuthMode::Signed, verified_kid.as_deref(), client_ip)
        }
        AuthMode::Public => resolve_rate_limit_key(&AuthMode::Public, None, client_ip),
    };

    // 4. Authenticated Rate Limiting (by verified kid in signed mode, by client IP in public mode)
    if let Err(e) = state.rate_limiter.check_global(&rate_key) {
        return Err((e, src_sha));
    }

    // 5. Deterministic Format & Cache Key Resolution BEFORE Cache Lookup
    let accept_header = headers.get(ACCEPT).and_then(|v| v.to_str().ok());
    let cache_format = resolve_cache_format(params.format, accept_header);
    let default_fmt = match cache_format {
        CacheFormat::Specific(f) => f,
        CacheFormat::Auto(AcceptClass::Avif) => ImageFormat::Avif,
        CacheFormat::Auto(AcceptClass::Webp) => ImageFormat::WebP,
        CacheFormat::Auto(AcceptClass::Legacy) => ImageFormat::Jpeg,
    };
    let quality = params
        .q
        .unwrap_or_else(|| resolve_quality(default_fmt, params.quality_target));

    let cache_key = compute_cache_key(&CacheKeyParams {
        src: &params.src,
        w: params.w,
        h: params.h,
        format: cache_format,
        quality,
        fit: params.fit,
        allow_upscale: params.allow_upscale,
        v: params.v.as_deref(),
    });

    // 6. Fast cache check BEFORE origin fetch (deterministically checks modern and legacy formats)
    if let Some(cached) = state.cache.get(&cache_key).await {
        if is_format_acceptable(cached.format, accept_header) {
            state.record_cache_hit(cached.bytes.len() as u64);
            let is_auto = matches!(params.format, Some(FormatParam::Auto) | None);
            let resp = build_response(
                state,
                cached,
                CacheStatus::Hit,
                &params,
                is_auto,
                ResponseTimings {
                    fetch_ms: None,
                    process_ms: None,
                    total_ms: start_time.elapsed().as_millis(),
                },
            );
            return Ok((resp, "hit", src_sha));
        }
    }

    // 7. Cache Miss: Check stricter rate limit for expensive fetch + transform work
    if let Err(e) = state.rate_limiter.check_miss(&rate_key) {
        return Err((e, src_sha));
    }

    // 8. Single-flight key is IDENTICAL to cache key (strict format and parameter isolation)
    let inflight_key = format!("inflight:{cache_key}");

    let follower_wait_timeout =
        std::time::Duration::from_secs(state.config.source_fetch_timeout_secs.max(10));
    let mut retry_count = 0;
    let (cached, is_auto, fetch_ms, process_ms) = loop {
        match state.inflight.subscribe_or_lead(&inflight_key) {
            InflightRole::Follower(mut rx) => {
                match tokio::time::timeout(follower_wait_timeout, rx.recv()).await {
                    Ok(Ok(Ok(cached))) => {
                        let is_auto = matches!(params.format, Some(FormatParam::Auto) | None);
                        break (cached, is_auto, None, None);
                    }
                    Ok(Ok(Err(InflightError::LeaderCancelled)))
                    | Ok(Err(tokio::sync::broadcast::error::RecvError::Closed))
                    | Err(_) => {
                        // Previous leader disconnected, was cancelled, or follower timed out
                        retry_count += 1;
                        if retry_count > 5 {
                            return Err((
                                AppError::Internal(
                                    "Inflight coordination failed after retries".to_string(),
                                ),
                                src_sha.clone(),
                            ));
                        }
                        tokio::task::yield_now().await;
                        continue;
                    }
                    Ok(Ok(Err(InflightError::Failed(app_err)))) => {
                        return Err(((*app_err).clone(), src_sha.clone()));
                    }
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                        continue;
                    }
                }
            }
            InflightRole::Leader(guard) => {
                let leader_res =
                    execute_transform_pipeline(state, &params, accept_header, cache_key.clone())
                        .await;

                match leader_res {
                    Ok(tuple) => {
                        guard.complete(Ok(tuple.0.clone()));
                        break (tuple.0, tuple.1, Some(tuple.2), Some(tuple.3));
                    }
                    Err(err) => {
                        guard.complete(Err(InflightError::Failed(Arc::new(err.clone()))));
                        return Err((err, src_sha));
                    }
                }
            }
        }
    };

    state.record_cache_miss(cached.bytes.len() as u64);
    let resp = build_response(
        state,
        cached,
        CacheStatus::Miss,
        &params,
        is_auto,
        ResponseTimings {
            fetch_ms,
            process_ms,
            total_ms: start_time.elapsed().as_millis(),
        },
    );
    Ok((resp, "miss", src_sha))
}

async fn execute_transform_pipeline(
    state: &Arc<AppState>,
    params: &TransformParams,
    accept_header: Option<&str>,
    cache_key: String,
) -> Result<(Arc<CachedImage>, bool, u128, u128), AppError> {
    // 4. Fetch source image via SourceFetcher (Phase 3 SSRF & DNS-pinned)
    let fetch_start = Instant::now();
    let source_bytes = state.fetcher.fetch(&params.src).await?;
    let fetch_ms = fetch_start.elapsed().as_millis();
    crate::observability::record_fetch_duration(fetch_start.elapsed().as_secs_f64());

    // 5. Probe header-only: validate dimensions & memory limits before full decode
    let info = state.processor.probe(&source_bytes)?;

    // Reject decompression bombs: dimension limits
    if info.width > state.config.max_source_dimension
        || info.height > state.config.max_source_dimension
    {
        return Err(AppError::ImageTooLarge(format!(
            "Source dimensions {}x{} exceed MAX_SOURCE_DIMENSION ({})",
            info.width, info.height, state.config.max_source_dimension
        )));
    }

    // Reject decompression bombs: estimated processing memory (w * h * channels * 3)
    let channels: u64 = if info.has_alpha { 4 } else { 3 };
    let estimated_mem_bytes = (info.width as u64) * (info.height as u64) * channels * 3;
    let max_processing_mem_bytes = (state.config.max_processing_memory_mb as u64) * 1024 * 1024;

    if estimated_mem_bytes > max_processing_mem_bytes {
        return Err(AppError::ImageTooLarge(format!(
            "Estimated processing memory ({:.1} MB) exceeds limit of {} MB",
            estimated_mem_bytes as f64 / (1024.0 * 1024.0),
            state.config.max_processing_memory_mb
        )));
    }

    // Resolve target format and quality
    let (resolved_format, is_auto) =
        resolve_format(params.format, accept_header, info.has_alpha, info.format);
    let resolved_quality = params
        .q
        .unwrap_or_else(|| resolve_quality(resolved_format, params.quality_target));

    // 6. Dimension clamping & non-upscaling
    let max_out_dim = state.config.max_output_dimension;
    let mut target_w = params.w.map(|w| w.min(max_out_dim));
    let mut target_h = params.h.map(|h| h.min(max_out_dim));

    if !params.allow_upscale {
        if let Some(w) = target_w {
            if w > info.width {
                target_w = Some(info.width);
            }
        }
        if let Some(h) = target_h {
            if h > info.height {
                target_h = Some(info.height);
            }
        }
    }

    // 7. Acquire concurrency semaphore and process image inside spawn_blocking
    let queue_guard = crate::state::TransformQueueGuard::enter(state);
    let permit = state
        .transform_semaphore
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| AppError::Internal(format!("Concurrency semaphore error: {e}")))?;
    let _active_guard = queue_guard.transition_to_active();

    let process_opts = ProcessOptions {
        target_w,
        target_h,
        fit: params.fit,
        format: match params.format {
            Some(FormatParam::Specific(fmt)) => Some(fmt),
            _ => None,
        },
        quality: Some(resolved_quality),
        quality_target: params.quality_target,
        accept_header: accept_header.map(|s| s.to_string()),
    };

    let processor = state.processor.clone();
    let process_start = Instant::now();

    let output = tokio::task::spawn_blocking(move || {
        let _held_permit = permit; // Permit is held for the duration of processing
        processor.process(&source_bytes, process_opts)
    })
    .await;

    let output = output
        .map_err(|e| AppError::Internal(format!("Worker thread panicked: {e}")))?
        .map_err(AppError::from)?;

    let process_ms = process_start.elapsed().as_millis();

    let cached = Arc::new(CachedImage {
        bytes: output.bytes,
        format: output.format,
        width: output.width,
        height: output.height,
    });

    // 8. Insert into cache using deterministic cache key
    let inserted = state
        .cache
        .insert_if_size_allowed(cache_key, cached.clone())
        .await;

    if !inserted {
        crate::observability::record_oversize_not_cached();
        tracing::debug!(
            bytes = cached.bytes.len(),
            "Output exceeds single-entry limit; served without caching"
        );
    }

    Ok((cached, is_auto, fetch_ms, process_ms))
}

fn build_response(
    state: &AppState,
    cached: Arc<CachedImage>,
    cache_status: CacheStatus,
    params: &TransformParams,
    is_auto: bool,
    timings: ResponseTimings,
) -> Response {
    let mut resp = (StatusCode::OK, Body::from(cached.bytes.clone())).into_response();
    let headers = resp.headers_mut();

    // Security header
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );

    // 1. Content-Type
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static(cached.format.content_type()),
    );

    // 2. Cache status header
    headers.insert(
        X_ATIHSRAH_CACHE.clone(),
        HeaderValue::from_static(cache_status.as_header_str()),
    );

    // 3. Cache-Control: clamp by remaining signature validity (exp) if present
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let remaining_ttl = params.exp.map(|exp| exp.saturating_sub(now_secs));

    let cache_control_val = match (params.v.is_some(), remaining_ttl) {
        (_, Some(0)) => "no-cache, no-store, must-revalidate".to_string(),
        (true, Some(ttl)) => {
            let max_age = std::cmp::min(31536000, ttl);
            format!("public, max-age={max_age}")
        }
        (true, None) => "public, max-age=31536000, immutable".to_string(),
        (false, Some(ttl)) => {
            let max_age = std::cmp::min(86400, ttl);
            format!("public, max-age={max_age}")
        }
        (false, None) => "public, max-age=86400".to_string(),
    };

    if let Ok(hv) = HeaderValue::from_str(&cache_control_val) {
        headers.insert(CACHE_CONTROL, hv);
    }

    // 4. Vary: Accept when format=auto
    if is_auto {
        headers.insert(VARY, HeaderValue::from_static("Accept"));
    }

    // 5. Measured debug timing headers (only if ATIHSRAH_DEBUG_HEADERS=true)
    if state.config.debug_headers {
        if let Some(f_ms) = timings.fetch_ms {
            if let Ok(hv) = HeaderValue::from_str(&f_ms.to_string()) {
                headers.insert(X_DEBUG_FETCH_TIME.clone(), hv);
            }
        }
        if let Some(p_ms) = timings.process_ms {
            if let Ok(hv) = HeaderValue::from_str(&p_ms.to_string()) {
                headers.insert(X_DEBUG_PROCESS_TIME.clone(), hv);
            }
        }
        if let Ok(hv) = HeaderValue::from_str(&timings.total_ms.to_string()) {
            headers.insert(X_DEBUG_TOTAL_TIME.clone(), hv);
        }
    }

    resp
}
