use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::state::AppState;

/// GET /health - Liveness probe: returns 200 OK unless the process is wedged
/// (e.g. internal watchdog tripped or runtime unresponsive). High load or full queue does NOT fail liveness.
pub async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if state.is_wedged() {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "wedged",
                "error": "Internal watchdog tripped or runtime unresponsive"
            })),
        )
    } else {
        (StatusCode::OK, Json(json!({ "status": "ok" })))
    }
}

/// GET /ready - Readiness probe: returns 200 OK when ready to accept traffic.
/// Returns 503 during startup, during graceful drain (after SIGTERM), or when saturated.
pub async fn ready_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.readiness_check() {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ready" }))),
        Err(reason) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": reason, "ready": false, "reason": reason })),
        ),
    }
}
