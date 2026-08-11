use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use super::{api_error, ApiResult, AppState};

pub(super) async fn test_message(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    state.osc.queue_test().map_err(|code| {
        let (status, detail) = match code {
            "osc.disabled" => (StatusCode::CONFLICT, "OSC chatbox output is disabled"),
            _ => (StatusCode::SERVICE_UNAVAILABLE, "OSC chatbox queue is full"),
        };
        api_error(status, code, detail)
    })?;
    Ok(Json(json!({ "queued": true })))
}
