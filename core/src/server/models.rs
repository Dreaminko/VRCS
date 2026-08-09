use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::asr;
use crate::error::AppError;

use super::{api_domain_error_with_params, api_error_with_params, ApiResult, AppState};

pub(super) async fn asr_capabilities(State(state): State<Arc<AppState>>) -> Json<Value> {
    let cuda = asr::cuda_capability();
    let active_model = state
        .config
        .read()
        .expect("config lock")
        .asr
        .local
        .model
        .clone();
    let (runtime_status, _) = state.asr_runtime.snapshot();
    let models = state
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
        "compute_types": {
            "auto": ["int8"],
            "cpu": ["int8"],
            "cuda": if cuda.available { vec!["int8"] } else { vec![] },
        },
        "models": models,
    }))
}

pub(super) async fn asr_models(State(state): State<Arc<AppState>>) -> Json<Value> {
    let active_model = state
        .config
        .read()
        .expect("config lock")
        .asr
        .local
        .model
        .clone();
    let (runtime_status, _) = state.asr_runtime.snapshot();
    Json(json!(state
        .model_manager
        .list(&active_model, runtime_status)))
}

pub(super) async fn asr_model_download(
    State(state): State<Arc<AppState>>,
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
    let manager = Arc::clone(&state.model_manager);
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
        .read()
        .expect("config lock")
        .asr
        .local
        .model
        .clone();
    let (runtime_status, _) = state.asr_runtime.snapshot();
    let record = state
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
    State(state): State<Arc<AppState>>,
    axum::extract::Path(model): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    let active_model = state
        .config
        .read()
        .expect("config lock")
        .asr
        .local
        .model
        .clone();
    if !asr::is_supported_model(&model) {
        return Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "asr.model.unsupported",
            json!({ "model": model }),
            format!("Unsupported recognition model: {model}"),
        ));
    }
    if model == active_model {
        return Err(api_error_with_params(
            StatusCode::CONFLICT,
            "asr.model.in_use",
            json!({ "model": model }),
            "This model is currently in use; select another model first",
        ));
    }
    state
        .model_manager
        .delete(&model, &active_model)
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
