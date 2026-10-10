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
    LearningError, LearningItem, LearningStatus, PatchLearningItem, SelectionQueryRequest,
};

use super::{api_error, api_error_with_params, db_call, ApiResult, ServiceContext};

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
    State(state): State<ServiceContext>,
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
    let items = learning_db_call(&state, move |db| {
        db.learning_items(limit, query.before_id, query.status)
    })
    .await
    .map_err(learning_db_error)?;
    Ok(Json(json!(items)))
}

pub(super) async fn learning_capture_keys(
    State(state): State<ServiceContext>,
) -> ApiResult<Json<Value>> {
    let keys = learning_db_call(&state, |db| db.learning_capture_keys())
        .await
        .map_err(learning_db_error)?;
    Ok(Json(json!({ "keys": keys })))
}

pub(super) async fn learning_item_create(
    State(state): State<ServiceContext>,
    Json(input): Json<CreateLearningItem>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    input.validate().map_err(learning_validation_error)?;
    let item = learning_db_call(&state, move |db| db.create_learning_item(input))
        .await
        .map_err(learning_db_error)?;
    Ok((StatusCode::CREATED, Json(json!(item))))
}

pub(super) async fn learning_item_patch(
    State(state): State<ServiceContext>,
    Path(id): Path<i64>,
    Json(patch): Json<PatchLearningItem>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    patch.validate().map_err(learning_validation_error)?;
    let item = learning_db_call(&state, move |db| db.patch_learning_item(id, patch))
        .await
        .map_err(learning_db_error)?
        .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(item)))
}

pub(super) async fn learning_item_archive(
    State(state): State<ServiceContext>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let item = learning_db_call(&state, move |db| db.archive_learning_item(id))
        .await
        .map_err(learning_db_error)?
        .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(item)))
}

pub(super) async fn learning_item_restore(
    State(state): State<ServiceContext>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let item = learning_db_call(&state, move |db| db.restore_learning_item(id))
        .await
        .map_err(learning_db_error)?
        .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(item)))
}

pub(super) async fn learning_item_delete(
    State(state): State<ServiceContext>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let deleted = learning_db_call(&state, move |db| db.delete_learning_item(id))
        .await
        .map_err(learning_db_error)?;
    if !deleted {
        return Err(learning_not_found(id));
    }
    Ok(Json(json!({ "deleted": true })))
}

pub(super) async fn learning_item_analyze(
    State(state): State<ServiceContext>,
    Path(id): Path<i64>,
    Json(request): Json<AnalyzeLearningItemRequest>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    request.validate().map_err(learning_validation_error)?;
    let item = load_editable_item(&state, id).await?;
    let profiles = state
        .config
        .config
        .read()
        .expect("config lock")
        .asr
        .api_profiles
        .clone();
    let analysis = state
        .content
        .learning_service
        .analyze(&item, &profiles, &request)
        .await
        .map_err(learning_service_error)?;
    let saved = learning_db_call(&state, move |db| db.save_learning_analysis(id, analysis))
        .await
        .map_err(learning_db_error)?
        .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(saved)))
}

pub(super) async fn selection_query(
    State(state): State<ServiceContext>,
    Json(request): Json<SelectionQueryRequest>,
) -> ApiResult<Json<Value>> {
    request.validate().map_err(learning_validation_error)?;
    let profiles = state
        .config
        .config
        .read()
        .expect("config lock")
        .asr
        .api_profiles
        .clone();
    let response = state
        .content
        .learning_service
        .ask_selection(&profiles, &request)
        .await
        .map_err(learning_service_error)?;
    Ok(Json(json!(response)))
}

pub(super) async fn learning_item_draft(
    State(state): State<ServiceContext>,
    Path(id): Path<i64>,
    Json(request): Json<CreateLearningDraftRequest>,
) -> ApiResult<Json<Value>> {
    validate_id(id)?;
    let item = load_editable_item(&state, id).await?;
    let draft = generate_draft(&item, request.card_type).map_err(learning_service_error)?;
    let saved = learning_db_call(&state, move |db| db.save_learning_draft(id, draft))
        .await
        .map_err(learning_db_error)?
        .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(saved)))
}

pub(super) async fn learning_item_export(
    State(state): State<ServiceContext>,
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
    let anki_config = state
        .config
        .config
        .read()
        .expect("config lock")
        .anki
        .clone();
    let card = draft.card_request();
    let mut features = state.config.features_tx.subscribe();
    let note_id =
        anki_service::create_card(&state.integrations.http, &card, &anki_config, async move {
            let _ = features
                .wait_for(|features| !features.anki || !features.learning)
                .await;
        })
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::from_u16(error.status_code).unwrap_or(StatusCode::BAD_GATEWAY),
                format!("anki.{}", error.code),
                error.params,
                error.message,
            )
        })?;
    let saved = db_call(Arc::clone(&state.content.db), move |db| {
        db.save_learning_export(id, note_id)
    })
    .await
    .map_err(learning_db_error)?
    .ok_or_else(|| learning_not_found(id))?;
    Ok(Json(json!(saved)))
}

async fn learning_db_call<T, F>(state: &ServiceContext, operation: F) -> crate::error::AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut crate::db::Database) -> crate::error::AppResult<T> + Send + 'static,
{
    let features = state.config.features_tx.subscribe();
    db_call(Arc::clone(&state.content.db), move |db| {
        if !features.borrow().learning {
            return Err(AppError::Conflict("Learning is disabled".into()));
        }
        operation(db)
    })
    .await
}

async fn load_item(state: &ServiceContext, id: i64) -> ApiResult<LearningItem> {
    learning_db_call(state, move |db| db.learning_item(id))
        .await
        .map_err(learning_db_error)?
        .ok_or_else(|| learning_not_found(id))
}

async fn load_editable_item(state: &ServiceContext, id: i64) -> ApiResult<LearningItem> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn feature_switches_recheck_learning_before_queued_database_work() {
        let directory = tempfile::tempdir().unwrap();
        let handle = crate::start(crate::CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: Some("queue-test".into()),
            vad_model_path: Some(directory.path().join("missing.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let state = ServiceContext::from_app(&handle.state);
        let db = state.content.db.clone();
        let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _lock = db.lock().unwrap();
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        locked_rx.await.unwrap();
        let (queued_tx, queued_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            queued_tx.send(()).unwrap();
            learning_db_call(&state, |_| {
                panic!("Disabled learning work must not reach the database")
            })
            .await
        });
        queued_rx.await.unwrap();
        handle
            .state
            .config
            .features_tx
            .send_modify(|features| features.learning = false);
        release_tx.send(()).unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err::<(), _>(AppError::Conflict(_))
        ));
        blocker.await.unwrap();
        handle.shutdown().await.unwrap();
    }
}
