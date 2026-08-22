use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use super::{api_error, ApiResult, IntegrationState};

pub(super) async fn test_message(State(state): State<IntegrationState>) -> ApiResult<Json<Value>> {
    state.osc.queue_test().map_err(|code| {
        let (status, detail) = match code {
            "osc.disabled" => (StatusCode::CONFLICT, "OSC chatbox output is disabled"),
            "osc.blocked_vrchat_muted" => (
                StatusCode::CONFLICT,
                "OSC chatbox output is blocked because VRChat is muted",
            ),
            "osc.blocked_mute_unknown" => (
                StatusCode::CONFLICT,
                "OSC chatbox output is blocked until the VRChat mute state is known",
            ),
            _ => (StatusCode::SERVICE_UNAVAILABLE, "OSC chatbox queue is full"),
        };
        api_error(status, code, detail)
    })?;
    Ok(Json(json!({ "queued": true })))
}
