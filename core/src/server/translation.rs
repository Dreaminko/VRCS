use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::translation::{TranslationError, TranslationEvent};

use super::{api_error, api_error_with_params, db_call, ApiResult, AppState};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewInput {
    text: String,
    #[serde(default)]
    source_language: Option<String>,
    #[serde(default)]
    target_language: Option<String>,
}

pub(super) async fn translation_preview(
    State(state): State<Arc<AppState>>,
    Json(input): Json<PreviewInput>,
) -> ApiResult<Json<Value>> {
    let config = state.config.read().expect("config lock").clone();
    let result = state
        .translation_service
        .translate(
            &config.translation,
            &config.asr.api_profiles,
            &input.text,
            input.source_language.as_deref(),
            input.target_language.as_deref(),
        )
        .await
        .map_err(translation_error)?;
    Ok(Json(json!(result.into_record())))
}

pub(super) async fn subtitle_translate(
    State(state): State<Arc<AppState>>,
    Path(subtitle_id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let subtitle = db_call(Arc::clone(&state.db), move |db| db.subtitle(subtitle_id))
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "translation.subtitle_load_failed",
                error.to_string(),
            )
        })?
        .ok_or_else(|| {
            api_error_with_params(
                StatusCode::NOT_FOUND,
                "translation.subtitle_not_found",
                json!({ "subtitle_id": subtitle_id }),
                "Subtitle not found",
            )
        })?;
    let config = state.config.read().expect("config lock").clone();
    if config.translation.mode == "disabled" {
        return Err(api_error(
            StatusCode::CONFLICT,
            "translation.disabled",
            "Translation is disabled",
        ));
    }
    let _ = state
        .translation_tx
        .send(TranslationEvent::TranslationStarted { subtitle_id });
    let record = state
        .translation_service
        .translate(
            &config.translation,
            &config.asr.api_profiles,
            &subtitle.text,
            subtitle.language.as_deref(),
            None,
        )
        .await
        .map_err(translation_error)?
        .into_record();
    let saved = record.clone();
    db_call(Arc::clone(&state.db), move |db| {
        db.save_translation(subtitle_id, &saved)
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "translation.storage_failed",
            error.to_string(),
        )
    })?;
    let _ = state
        .translation_tx
        .send(TranslationEvent::TranslationCompleted {
            subtitle_id,
            translation: record.clone(),
        });
    Ok(Json(json!(record)))
}

fn translation_error(error: TranslationError) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "translation.invalid_text"
        | "translation.invalid_target_language"
        | "translation.unsupported_provider" => StatusCode::UNPROCESSABLE_ENTITY,
        "translation.not_configured"
        | "translation.credential_missing"
        | "translation.disabled" => StatusCode::CONFLICT,
        "translation.authentication_failed"
        | "translation.credential_failed"
        | "llm.authentication_failed" => StatusCode::UNAUTHORIZED,
        "translation.rate_limited" | "translation.quota_exceeded" | "llm.rate_limited" => {
            StatusCode::TOO_MANY_REQUESTS
        }
        "translation.timeout" | "llm.timeout" => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::BAD_GATEWAY,
    };
    api_error(status, error.code, error.detail)
}
