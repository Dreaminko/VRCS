use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::asr;

use super::{api_error, ApiResult, AppState};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CredentialInput {
    api_key: String,
}

pub(super) async fn credential_statuses() -> ApiResult<Json<Value>> {
    let qwen = asr::credential_status("qwen").map_err(|error| credential_error("qwen", error))?;
    let openai =
        asr::credential_status("openai").map_err(|error| credential_error("openai", error))?;
    Ok(Json(json!({ "qwen": qwen, "openai": openai })))
}

pub(super) async fn credential_write(
    Path(provider): Path<String>,
    Json(input): Json<CredentialInput>,
) -> ApiResult<Json<Value>> {
    asr::write_credential(&provider, &input.api_key)
        .map_err(|error| credential_error(&provider, error))?;
    let status =
        asr::credential_status(&provider).map_err(|error| credential_error(&provider, error))?;
    Ok(Json(json!(status)))
}

pub(super) async fn credential_delete(Path(provider): Path<String>) -> ApiResult<Json<Value>> {
    asr::delete_credential(&provider).map_err(|error| credential_error(&provider, error))?;
    Ok(Json(json!({ "configured": false, "source": null })))
}

pub(super) async fn credential_test(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
) -> ApiResult<Json<Value>> {
    let config = state.config.read().expect("config lock").asr.clone();
    asr::test_streaming_connection(&config, &provider)
        .await
        .map_err(|error| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "asr.cloud_test_failed",
                error,
            )
        })?;
    Ok(Json(json!({ "ok": true })))
}

fn credential_error(provider: &str, detail: String) -> (StatusCode, Json<Value>) {
    let status = if detail.contains("不支持") || detail.contains("长度") {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    api_error(
        status,
        "asr.credential_failed",
        format!("{provider}: {detail}"),
    )
}
