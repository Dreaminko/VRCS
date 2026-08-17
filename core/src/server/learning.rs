use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::anki as anki_service;
use crate::error::AppError;
use crate::learning::{
    generate_draft, AnalyzeLearningItemRequest, CreateLearningDraftRequest, CreateLearningItem,
    LearningError, LearningItem, LearningStatus, PatchLearningItem,
};

use super::{api_error, api_error_with_params, db_call, ApiResult, AppState};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LearningListQuery {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    before_id: Option<i64>,
    #[serde(default)]
    status: Option<LearningStatus>,
}

pub(super) async fn learning_items(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LearningListQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.unwrap_or(100);
    if !(1..=500).contains(&limit) {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "learning.invalid_limit",
            "limit must be between 1 and 500",
        ));
    }
    if query.before_id.is_some_and(|id| id <= 0) {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "learning.invalid_before_id",
            "before_id must be a positive integer",
        ));
    }
    let items = db_call(Arc::clone(&state.db), move |db| {
        db.learning_items(limit, query.before_id, query.status)
    })
    .await
    .map_err(learning_db_error)?;
    Ok(Json(json!(items)))
}

pub(super) async fn learning_capture_keys(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Value>> {
    let keys = db_call(Arc::clone(&state.db), |db| db.learning_capture_keys())
        .await
        .map_err(learning_db_error)?;
    Ok(Json(json!({ "keys": keys })))
}

pub(super) async fn learning_item_create(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateLearningItem>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    input.validate().map_err(learning_validation_error)?;
    let item = db_call(Arc::clone(&state.db), move |db| {
        db.create_learning_item(input)
    })
    .await
    .map_err(learning_db_error)?;
    Ok((StatusCode::CREATED, Json(json!(item))))
}

pub(super) async fn learning_item_patch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(patch): Json<PatchLearningItem>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    patch.validate().map_err(learning_validation_error)?;
    let item = db_call(Arc::clone(&state.db), move |db| {
        db.patch_learning_item(id, patch)
    })
    .await
    .map_err(learning_db_error)?
    .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(item)))
}

pub(super) async fn learning_item_archive(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let item = db_call(Arc::clone(&state.db), move |db| {
        db.archive_learning_item(id)
    })
    .await
    .map_err(learning_db_error)?
    .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(item)))
}

pub(super) async fn learning_item_restore(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let item = db_call(Arc::clone(&state.db), move |db| {
        db.restore_learning_item(id)
    })
    .await
    .map_err(learning_db_error)?
    .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(item)))
}

pub(super) async fn learning_item_delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let deleted = db_call(Arc::clone(&state.db), move |db| db.delete_learning_item(id))
        .await
        .map_err(learning_db_error)?;
    if !deleted {
        return Err(learning_not_found(id));
    }
    Ok(Json(json!({ "deleted": true })))
}

pub(super) async fn learning_item_analyze(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(request): Json<AnalyzeLearningItemRequest>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    request.validate().map_err(learning_validation_error)?;
    let item = load_editable_item(&state, id).await?;
    let profiles = state
        .config
        .read()
        .expect("config lock")
        .asr
        .api_profiles
        .clone();
    let analysis = state
        .learning_service
        .analyze(&item, &profiles, &request)
        .await
        .map_err(learning_service_error)?;
    let saved = db_call(Arc::clone(&state.db), move |db| {
        db.save_learning_analysis(id, analysis)
    })
    .await
    .map_err(learning_db_error)?
    .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(saved)))
}

pub(super) async fn learning_item_draft(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(request): Json<CreateLearningDraftRequest>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let item = load_editable_item(&state, id).await?;
    let draft = generate_draft(&item, request.card_type).map_err(learning_service_error)?;
    let saved = db_call(Arc::clone(&state.db), move |db| {
        db.save_learning_draft(id, draft)
    })
    .await
    .map_err(learning_db_error)?
    .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(saved)))
}

pub(super) async fn learning_item_export(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let item = load_editable_item(&state, id).await?;
    if item.anki_note_id.is_some() {
        return Ok(Json(json!(item)));
    }
    let draft = item.draft.as_ref().ok_or_else(|| {
        api_error_with_params(
            StatusCode::CONFLICT,
            "learning.draft_missing",
            json!({ "id": id }),
            "Create and save a learning card draft before export",
        )
    })?;
    let anki_config = state.config.read().expect("config lock").anki.clone();
    let card = draft.card_request();
    let note_id = anki_service::create_card(&state.http, &card, &anki_config)
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::from_u16(error.status_code).unwrap_or(StatusCode::BAD_GATEWAY),
                format!("anki.{}", error.code),
                error.params,
                error.message,
            )
        })?;
    let saved = db_call(Arc::clone(&state.db), move |db| {
        db.save_learning_export(id, note_id)
    })
    .await
    .map_err(learning_db_error)?
    .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(saved)))
}

async fn load_item(state: &AppState, id: i64) -> ApiResult<LearningItem> {
    db_call(Arc::clone(&state.db), move |db| db.learning_item(id))
        .await
        .map_err(learning_db_error)?
        .ok_or_else(|| learning_not_found(id))
}

async fn load_editable_item(state: &AppState, id: i64) -> ApiResult<LearningItem> {
    let item = load_item(state, id).await?;
    if item.status == LearningStatus::Archived {
        return Err(api_error_with_params(
            StatusCode::CONFLICT,
            "learning.archived",
            json!({ "id": id }),
            "Restore the archived learning item before modifying it",
        ));
    }
    Ok(item)
}

fn validate_id(id: i64) -> ApiResult<()> {
    if id <= 0 {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "learning.invalid_id",
            "Learning item ID must be positive",
        ));
    }
    Ok(())
}

fn learning_not_found(id: i64) -> (StatusCode, Json<Value>) {
    api_error_with_params(
        StatusCode::NOT_FOUND,
        "learning.not_found",
        json!({ "id": id }),
        "Learning item does not exist",
    )
}

fn learning_validation_error(detail: String) -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "learning.invalid_request",
        detail,
    )
}

fn learning_db_error(error: AppError) -> (StatusCode, Json<Value>) {
    match error {
        AppError::Validation(detail) => learning_validation_error(detail),
        AppError::Conflict(detail) => api_error(StatusCode::CONFLICT, "learning.conflict", detail),
        other => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "learning.storage_failed",
            other.to_string(),
        ),
    }
}

fn learning_service_error(error: LearningError) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "learning.invalid_request"
        | "learning.not_configured"
        | "learning.unsupported_provider"
        | "learning.credential_missing"
        | "learning.credential_failed"
        | "learning.invalid_configuration"
        | "learning.draft_invalid"
        | "learning.draft_unavailable" => StatusCode::UNPROCESSABLE_ENTITY,
        "learning.authentication_failed" => StatusCode::UNAUTHORIZED,
        "learning.rate_limited" => StatusCode::TOO_MANY_REQUESTS,
        "learning.timeout" => StatusCode::GATEWAY_TIMEOUT,
        "learning.provider_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        "learning.invalid_response" => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_GATEWAY,
    };
    api_error_with_params(
        status,
        error.code,
        json!({ "retryable": error.retryable }),
        error.detail,
    )
}
