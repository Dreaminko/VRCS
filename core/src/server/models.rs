use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::asr;
use crate::error::AppError;

use super::{
    api_domain_error_with_params, api_error_with_params, ApiResult, ModelContext, SettingsContext,
};

pub(super) async fn asr_capabilities(State(state): State<ModelContext>) -> Json<Value> {
    let cuda = asr::cuda_capability();
    let vulkan = asr::vulkan_capability();
    let active_model = state
        .config
        .config
        .read()
        .expect("config lock")
        .asr
        .local
        .model
        .clone();
    let (runtime_status, _) = state.capture.asr_runtime.snapshot();
    let models = state
        .capture
        .model_manager
        .list(&active_model, runtime_status)
        .into_iter()
        .map(|model| {
            let status = match model.status.as_str() {
                "downloaded" | "loading" | "ready" | "error" => model.status,
                _ => "not_downloaded".into(),
            };
            json!({
                "id": model.id,
                "repository": model.repository,
                "status": status,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "runtime_available": true,
        "cuda": cuda,
        "vulkan": vulkan,
        "compute_types": {
            "auto": ["int8"],
            "cpu": ["int8"],
            "cuda": if cuda.available { vec!["int8"] } else { vec![] },
            "vulkan": if vulkan.available { vec!["int8"] } else { vec![] },
        },
        "models": models,
    }))
}

pub(super) async fn asr_models(State(state): State<ModelContext>) -> Json<Value> {
    let active_model = state
        .config
        .config
        .read()
        .expect("config lock")
        .asr
        .local
        .model
        .clone();
    let (runtime_status, _) = state.capture.asr_runtime.snapshot();
    Json(json!(state
        .capture
        .model_manager
        .list(&active_model, runtime_status)))
}

pub(super) async fn asr_model_download(
    State(state): State<ModelContext>,
    axum::extract::Path(model): axum::extract::Path<String>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if !asr::is_supported_model(&model) {
        return Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "asr.model.unsupported",
            json!({ "model": model }),
            format!("Unsupported recognition model: {model}"),
        ));
    }
    let manager = Arc::clone(&state.capture.model_manager);
    let download_model = model.clone();
    tokio::task::spawn_blocking(move || manager.start_download(&download_model))
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.model.download_task_failed",
                json!({ "model": model }),
                format!("Model download startup task failed: {error}"),
            )
        })?
        .map_err(|error| {
            api_domain_error_with_params(
                AppError::Conflict(error),
                "asr.model.download_conflict",
                json!({ "model": model }),
            )
        })?;
    let active_model = state
        .config
        .config
        .read()
        .expect("config lock")
        .asr
        .local
        .model
        .clone();
    let (runtime_status, _) = state.capture.asr_runtime.snapshot();
    let record = state
        .capture
        .model_manager
        .describe(&model, &active_model, runtime_status)
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.model.describe_failed",
                json!({ "model": model }),
                error,
            )
        })?;
    Ok((StatusCode::ACCEPTED, Json(json!(record))))
}

pub(super) async fn asr_model_delete(
    State(state): State<SettingsContext>,
    axum::extract::Path(model): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    let _control = state.config.config_control.lock().await;
    let mut candidate = state.config.config.read().expect("config lock").clone();
    if !asr::is_supported_model(&model) {
        return Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "asr.model.unsupported",
            json!({ "model": model }),
            format!("Unsupported recognition model: {model}"),
        ));
    }
    if model == candidate.asr.local.model {
        candidate.asr.local.model.clear();
        candidate = fallback_after_delete(&state, candidate, &model).await?;
        super::settings::commit_candidate(&state, candidate.clone()).await?;
    }
    state
        .capture
        .model_manager
        .delete(&model, &candidate.asr.local.model)
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.model.delete_failed",
                json!({ "model": model }),
                error,
            )
        })?;
    Ok(Json(json!({ "deleted": true })))
}

pub(super) async fn fallback_after_delete(
    state: &SettingsContext,
    mut candidate: crate::config::AppConfig,
    removed: &str,
) -> ApiResult<crate::config::AppConfig> {
    let manager = Arc::clone(&state.capture.model_manager);
    let removed = removed.to_owned();
    tokio::task::spawn_blocking(move || {
        let whisper_models = manager
            .list("", "not_loaded")
            .into_iter()
            .filter(|model| model.id != removed && model.status == "downloaded")
            .collect::<Vec<_>>();
        let whisper = whisper_models
            .iter()
            .find(|model| model.id == candidate.asr.local.model)
            .or_else(|| whisper_models.first())
            .cloned()
            .map(|model| model.id);
        let qwen = manager
            .list_qwen()?
            .into_iter()
            .find(|model| model.id != removed && model.status == "installed")
            .map(|model| model.id);
        match candidate.asr.backend.as_str() {
            "local_whisper" if asr::is_supported_model(&removed) => {
                candidate.asr.local.model = whisper.unwrap_or_default();
                if candidate.asr.local.model.is_empty() {
                    if let Some(package) = qwen {
                        candidate.asr.backend = crate::config::QWEN_MANAGED_BACKEND.into();
                        candidate.asr.managed_qwen.package_id = package;
                    }
                }
            }
            crate::config::QWEN_MANAGED_BACKEND if !asr::is_supported_model(&removed) => {
                candidate.asr.managed_qwen.package_id = qwen.unwrap_or_default();
                if candidate.asr.managed_qwen.package_id.is_empty() {
                    if let Some(model) = whisper {
                        candidate.asr.backend = "local_whisper".into();
                        candidate.asr.local.model = model;
                    }
                }
            }
            _ => {
                if candidate.asr.local.model.is_empty() {
                    candidate.asr.local.model = whisper.unwrap_or_default();
                }
                if candidate.asr.managed_qwen.package_id.is_empty() {
                    candidate.asr.managed_qwen.package_id = qwen.unwrap_or_default();
                }
            }
        }
        Ok::<_, String>(candidate)
    })
    .await
    .map_err(|error| {
        super::api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "asr.model.inspect_task_failed",
            error.to_string(),
        )
    })?
    .map_err(|error| {
        super::api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "asr.model.inspect_failed",
            error,
        )
    })
}
