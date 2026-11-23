use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{ApiResult, IntegrationState, api_error};

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

pub(super) async fn test_unitext(State(state): State<IntegrationState>) -> ApiResult<Json<Value>> {
    state.unitext.queue_test().map_err(map_unitext_error)?;
    Ok(Json(json!({ "queued": true })))
}

pub(super) async fn clear_unitext(State(state): State<IntegrationState>) -> ApiResult<Json<Value>> {
    state.unitext.clear().map_err(map_unitext_error)?;
    Ok(Json(json!({ "cleared": true })))
}

fn map_unitext_error(code: &'static str) -> (StatusCode, Json<Value>) {
    let (status, detail) = match code {
        "unitext.disabled" => (StatusCode::CONFLICT, "UniText prop output is disabled"),
        "unitext.not_detected" => (
            StatusCode::CONFLICT,
            "UniText avatar parameters have not been detected yet",
        ),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            "UniText command queue is full",
        ),
    };
    api_error(status, code, detail)
}
