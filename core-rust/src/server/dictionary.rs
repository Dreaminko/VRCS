use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use super::{
    api_error, api_error_with_params, db_call, dictionary_import_error, ApiResult, AppState,
};

pub(super) async fn subtitle_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let limit = match params.get("limit") {
        None => 500,
        Some(raw) => raw
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=500).contains(value))
            .ok_or_else(|| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subtitles.invalid_limit",
                    "limit 必须在 1 到 500 之间",
                )
            })?,
    };
    let history_limit = state
        .config
        .read()
        .expect("config lock")
        .storage
        .subtitle_history_limit;
    let subtitles = db_call(Arc::clone(&state.db), move |db| {
        db.subtitle_history(limit.min(history_limit))
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "subtitles.history_failed",
            error.to_string(),
        )
    })?;
    Ok(Json(json!(subtitles)))
}

pub(super) async fn dictionary_lookup(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let query = params
        .get("q")
        .filter(|value| !value.is_empty() && value.chars().count() <= 100)
        .ok_or_else(|| {
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "dictionary.invalid_query",
                "q 必须在 1 到 100 字符之间",
            )
        })?;
    let query = query.clone();
    let entries = db_call(Arc::clone(&state.db), move |db| db.lookup(&query, 10))
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dictionary.lookup_failed",
                error.to_string(),
            )
        })?;
    Ok(Json(json!(entries)))
}

pub(super) async fn dictionary_list(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let sources = db_call(Arc::clone(&state.db), |db| db.dictionary_sources())
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dictionary.list_failed",
                error.to_string(),
            )
        })?;
    Ok(Json(json!(sources)))
}

pub(super) async fn dictionary_import(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let imported = db_call(Arc::clone(&state.db), move |db| db.import_yomitan(&body))
        .await
        .map_err(dictionary_import_error)?;
    Ok(Json(json!(imported)))
}

pub(super) async fn dictionary_delete(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(source_id): axum::extract::Path<i64>,
) -> ApiResult<Json<Value>> {
    let deleted = db_call(Arc::clone(&state.db), move |db| {
        db.delete_dictionary_source(source_id)
    })
    .await
    .map_err(|error| {
        api_error_with_params(
            StatusCode::INTERNAL_SERVER_ERROR,
            "dictionary.delete_failed",
            json!({ "source_id": source_id }),
            error.to_string(),
        )
    })?;
    if !deleted {
        return Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "dictionary.not_found",
            json!({ "source_id": source_id }),
            "词典不存在",
        ));
    }
    Ok(Json(json!({ "deleted": true })))
}
