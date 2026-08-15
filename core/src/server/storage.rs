use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use super::{api_error, db_call, ApiResult, AppState};

pub(super) async fn database_stats(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let stats = db_call(Arc::clone(&state.db), |db| db.storage_stats())
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage.stats_failed",
                error.to_string(),
            )
        })?;
    Ok(Json(json!(stats)))
}

pub(super) async fn clear_subtitle_history(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Value>> {
    let stats = db_call(Arc::clone(&state.db), |db| db.clear_subtitle_history())
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage.clear_failed",
                error.to_string(),
            )
        })?;
    Ok(Json(json!(stats)))
}
