use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::asr;
use crate::error::AppError;

use super::{api_domain_error_with_params, api_error_with_params, ApiResult, ModelContext};

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
    let available = asr::qwen_executable_path().is_ok_and(|path| path.is_file());
    let running = state
        .capture
        .qwen_runtime
        .connection()
        .await
        .ok()
        .flatten()
        .is_some();
    Json(json!({ "available": available, "running": running }))
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
    State(state): State<ModelContext>,
    Path(package): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_package(&package)?;
    let selected = {
        let config = state.config.config.read().expect("config lock");
        config.asr.backend == crate::config::QWEN_MANAGED_BACKEND
            && config.asr.managed_qwen.package_id == package
    };
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
    if selected || running {
        return Err(api_error_with_params(
            StatusCode::CONFLICT,
            "asr.qwen_model.in_use",
            json!({ "model": package }),
            "This Qwen ASR package is currently in use; select another backend or package first",
        ));
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
