use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::db::conversations::publish_latest_catalog;

use super::{ApiResult, ContentState, api_error, db_call};

pub(super) async fn database_stats(State(state): State<ContentState>) -> ApiResult<Json<Value>> {
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
    State(state): State<ContentState>,
) -> ApiResult<Json<Value>> {
    let conversation_catalog = state.conversation_catalog_tx.clone();
    let stats = db_call(Arc::clone(&state.db), move |db| {
        let stats = db.clear_subtitle_history()?;
        publish_latest_catalog(db, &conversation_catalog);
        Ok(stats)
    })
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
