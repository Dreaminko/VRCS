use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{api_error, ApiResult, ServiceContext};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DownloadInput {
    id: String,
    revision: String,
}

pub(super) async fn status(State(state): State<ServiceContext>) -> ApiResult<Json<Value>> {
    Ok(Json(status_value(&state).await))
}

pub(super) async fn download(
    State(state): State<ServiceContext>,
    Json(input): Json<DownloadInput>,
) -> ApiResult<Json<Value>> {
    crate::platform_capabilities::require_local_translation().map_err(|code| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            code,
            "Local PLaMo translation is not implemented for this platform",
        )
    })?;
    require_model(&input.id)?;
    let models = Arc::clone(&state.content.local_translation_models);
    let lifecycle = models.lock_exclusive().await;
    state
        .content
        .plamo_runtime
        .stop()
        .await
        .map_err(storage_error)?;
    if let Err(detail) = models.start_download_locked(&input.revision, lifecycle) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "translation.local_download_failed",
            detail,
        ));
    }
    Ok(Json(status_value(&state).await))
}

pub(super) async fn cancel(State(state): State<ServiceContext>) -> ApiResult<Json<Value>> {
    state.content.local_translation_models.cancel().await;
    Ok(Json(status_value(&state).await))
}

pub(super) async fn verify(State(state): State<ServiceContext>) -> ApiResult<Json<Value>> {
    let models = Arc::clone(&state.content.local_translation_models);
    let lifecycle = models.lock_shared().await;
    tokio::task::spawn_blocking(move || models.verify_locked(&lifecycle))
        .await
        .map_err(|error| storage_error(error.to_string()))?
        .map_err(|detail| {
            api_error(
                StatusCode::CONFLICT,
                "translation.local_verify_failed",
                detail,
            )
        })?;
    Ok(Json(status_value(&state).await))
}

pub(super) async fn unload(State(state): State<ServiceContext>) -> ApiResult<Json<Value>> {
    state
        .content
        .plamo_runtime
        .stop()
        .await
        .map_err(storage_error)?;
    Ok(Json(status_value(&state).await))
}

pub(super) async fn delete(
    State(state): State<ServiceContext>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_model(&id)?;
    state.content.local_translation_models.cancel().await;
    let models = Arc::clone(&state.content.local_translation_models);
    let lifecycle = models.lock_exclusive().await;
    state
        .content
        .plamo_runtime
        .stop()
        .await
        .map_err(storage_error)?;
    let deleted = tokio::task::spawn_blocking(move || models.delete_locked(&lifecycle))
        .await
        .map_err(|error| storage_error(error.to_string()))?
        .map_err(|detail| {
            api_error(
                StatusCode::CONFLICT,
                "translation.local_delete_failed",
                detail,
            )
        })?;
    Ok(Json(json!({
        "deleted": deleted,
        "local_translation": status_value(&state).await,
    })))
}

async fn status_value(state: &ServiceContext) -> Value {
    json!({
        "model": state.content.local_translation_models.status(),
        "runtime": state.content.plamo_runtime.snapshot().await,
        "languages": crate::translation::SUPPORTED_LANGUAGE_CODES.as_slice(),
        "license": {
            "name": "PLaMo Community License",
            "url": "https://plamo.preferredai.jp/info/plamo-community-license-en",
            "attribution": "Built with PLaMo"
        }
    })
}

fn require_model(id: &str) -> ApiResult<()> {
    if id == crate::translation::MODEL_ID {
        Ok(())
    } else {
        Err(api_error(
            StatusCode::NOT_FOUND,
            "translation.local_model_not_found",
            "Unknown local translation model",
        ))
    }
}

fn storage_error(detail: String) -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "translation.local_storage_failed",
        detail,
    )
}
