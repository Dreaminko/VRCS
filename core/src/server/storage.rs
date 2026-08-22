use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::conversations::publish_latest_catalog;

use super::{api_error, db_call, ApiResult, ContentState};

#[derive(Deserialize)]
pub(super) struct DeleteSubtitleRangeRequest {
    started_at: String,
    ended_at: Option<String>,
}

fn parse_range_timestamp(value: &str) -> Result<DateTime<Utc>, (StatusCode, Json<Value>)> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| {
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "subtitles.invalid_range",
                "Conversation timestamps must use RFC 3339 format",
            )
        })
}

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

pub(super) async fn delete_subtitle_range(
    State(state): State<ContentState>,
    Json(input): Json<DeleteSubtitleRangeRequest>,
) -> ApiResult<Json<Value>> {
    let started_at = parse_range_timestamp(&input.started_at)?;
    let ended_at = input
        .ended_at
        .as_deref()
        .map(parse_range_timestamp)
        .transpose()?;
    if ended_at
        .as_ref()
        .is_some_and(|ended_at| ended_at <= &started_at)
    {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "subtitles.invalid_range",
            "Conversation end must be later than its start",
        ));
    }
    let started_at = started_at.to_rfc3339_opts(SecondsFormat::Micros, true);
    let ended_at = ended_at.map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Micros, true));
    let conversation_catalog = state.conversation_catalog_tx.clone();
    let deleted = db_call(Arc::clone(&state.db), move |db| {
        let deleted = db.delete_subtitle_range(&started_at, ended_at.as_deref())?;
        publish_latest_catalog(db, &conversation_catalog);
        Ok(deleted)
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "subtitles.delete_failed",
            error.to_string(),
        )
    })?;
    Ok(Json(json!({ "deleted": deleted })))
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
