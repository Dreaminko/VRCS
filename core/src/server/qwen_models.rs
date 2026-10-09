use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::asr;
use crate::error::AppError;

use super::{
    api_domain_error_with_params, api_error_with_params, ApiResult, ModelContext, SettingsContext,
};

fn ensure_package(id: &str) -> ApiResult<()> {
    if asr::is_supported_qwen_package(id) {
        Ok(())
    } else {
        Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "asr.qwen_model.unsupported",
            json!({ "model": id }),
            format!("Unsupported Qwen ASR package: {id}"),
        ))
    }
}

pub(super) async fn runtime_status(State(state): State<ModelContext>) -> Json<Value> {
    let available = state
        .capture
        .qwen_runtime
        .executable_path()
        .is_ok_and(|path| path.is_file());
    let snapshot = state.capture.qwen_runtime.snapshot().await;
    let devices = state.capture.qwen_runtime.devices().await;
    let mut status = serde_json::to_value(&snapshot).expect("runtime snapshot serializes");
    status["available"] = json!(available);
    status["running"] = json!(snapshot.status == "ready");
    status["gpu_devices"] = json!(devices);
    status["installation"] = json!(state
        .capture
        .model_manager
        .qwen_runtime_installation(&state.capture.qwen_runtime));
    Json(status)
}

pub(super) async fn download_runtime(
    State(state): State<ModelContext>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    state
        .capture
        .model_manager
        .start_qwen_runtime_download(Arc::clone(&state.capture.qwen_runtime))
        .map_err(|error| {
            api_domain_error_with_params(
                AppError::Conflict(error),
                "asr.qwen_model.download_conflict",
                json!({ "model": "Qwen runtime" }),
            )
        })?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "started": true }))))
}

pub(super) async fn cancel_runtime(State(state): State<ModelContext>) -> Json<Value> {
    state
        .capture
        .model_manager
        .cancel_qwen_runtime_download()
        .await;
    Json(json!({ "cancelled": true }))
}

pub(super) async fn list(State(state): State<ModelContext>) -> ApiResult<Json<Value>> {
    let manager = Arc::clone(&state.capture.model_manager);
    let records = tokio::task::spawn_blocking(move || manager.list_qwen())
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.qwen_model.list_task_failed",
                json!({}),
                error.to_string(),
            )
        })?
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.qwen_model.list_failed",
                json!({}),
                error,
            )
        })?;
    Ok(Json(json!(records)))
}

pub(super) async fn download(
    State(state): State<ModelContext>,
    Path(package): Path<String>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_package(&package)?;
    let manager = Arc::clone(&state.capture.model_manager);
    let requested = package.clone();
    let record = tokio::task::spawn_blocking(move || {
        manager.start_qwen_download(&requested)?;
        manager.describe_qwen(&requested)
    })
    .await
    .map_err(|error| {
        api_error_with_params(
            StatusCode::INTERNAL_SERVER_ERROR,
            "asr.qwen_model.download_task_failed",
            json!({ "model": package }),
            error.to_string(),
        )
    })?
    .map_err(|error| {
        api_domain_error_with_params(
            AppError::Conflict(error),
            "asr.qwen_model.download_conflict",
            json!({ "model": package }),
        )
    })?;
    Ok((StatusCode::ACCEPTED, Json(json!(record))))
}

pub(super) async fn cancel(
    State(state): State<ModelContext>,
    Path(package): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_package(&package)?;
    state
        .capture
        .model_manager
        .cancel_qwen_download(&package)
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.qwen_model.cancel_failed",
                json!({ "model": package }),
                error,
            )
        })?;
    Ok(Json(json!({ "cancelled": true })))
}

pub(super) async fn verify(
    State(state): State<ModelContext>,
    Path(package): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_package(&package)?;
    let manager = Arc::clone(&state.capture.model_manager);
    let requested = package.clone();
    let record = tokio::task::spawn_blocking(move || manager.verify_qwen(&requested))
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.qwen_model.verify_task_failed",
                json!({ "model": package }),
                error.to_string(),
            )
        })?
        .map_err(|error| {
            api_domain_error_with_params(
                AppError::Conflict(error),
                "asr.qwen_model.verify_conflict",
                json!({ "model": package }),
            )
        })?;
    Ok(Json(json!(record)))
}

pub(super) async fn delete(
    State(state): State<SettingsContext>,
    Path(package): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_package(&package)?;
    let _control = state.config.config_control.lock().await;
    let mut candidate = state.config.config.read().expect("config lock").clone();
    if candidate.asr.managed_qwen.package_id == package {
        candidate.asr.managed_qwen.package_id.clear();
        candidate = super::models::fallback_after_delete(&state, candidate, &package).await?;
        super::settings::commit_candidate(&state, candidate).await?;
    }
    let running = state
        .capture
        .qwen_runtime
        .uses_package(&package)
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.qwen_model.runtime_check_failed",
                json!({ "model": package }),
                error,
            )
        })?;
    if running {
        state.capture.qwen_runtime.stop().await.map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.qwen_model.runtime_check_failed",
                json!({ "model": package }),
                error,
            )
        })?;
    }
    let manager = Arc::clone(&state.capture.model_manager);
    let requested = package.clone();
    tokio::task::spawn_blocking(move || manager.delete_qwen(&requested))
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.qwen_model.delete_task_failed",
                json!({ "model": package }),
                error.to_string(),
            )
        })?
        .map_err(|error| {
            api_domain_error_with_params(
                AppError::Conflict(error),
                "asr.qwen_model.delete_conflict",
                json!({ "model": package }),
            )
        })?;
    Ok(Json(json!({ "deleted": true })))
}
