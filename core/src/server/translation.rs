use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::conversations::publish_latest_catalog;
use crate::subtitle_output::TranslationFailure;
use crate::translation::TranslationError;
use crate::{
    config::{validate_translation_prompt, TranslationPromptConfig},
    translation::TranslationPromptBuilder,
};

use super::{api_error, api_error_with_params, db_call, ApiResult, ServiceContext};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewInput {
    text: String,
    #[serde(default)]
    source_language: Option<String>,
    #[serde(default)]
    target_language: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PromptPreviewInput {
    prompt: TranslationPromptConfig,
    #[serde(default)]
    source_language: Option<String>,
    #[serde(default)]
    target_language: Option<String>,
}

pub(super) async fn prompt_preview(
    State(state): State<ServiceContext>,
    Json(input): Json<PromptPreviewInput>,
) -> ApiResult<Json<Value>> {
    validate_translation_prompt(&input.prompt).map_err(|detail| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "translation.prompt_invalid",
            detail,
        )
    })?;
    let prompt_config = input.prompt.clone();
    let mut context = db_call(Arc::clone(&state.content.db), move |db| {
        db.recent_translation_context(&prompt_config, None)
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "translation.context_load_failed",
            error.to_string(),
        )
    })?;
    append_vrcx_context(&state, &mut context);
    let target = input.target_language.as_deref().unwrap_or("zh-Hans");
    let mut prompt = input.prompt;
    prompt.glossary = state.content.glossary.entries_for_llm();
    let preview = TranslationPromptBuilder::new(&prompt).build(
        input.source_language.as_deref(),
        target,
        &context,
        "",
    );
    Ok(Json(json!({
        "instructions": preview.instructions,
        "context_message_count": preview.context_message_count,
        "context_char_count": preview.context_char_count,
    })))
}

fn append_vrcx_context(
    state: &ServiceContext,
    context: &mut Vec<crate::translation::TranslationContextEntry>,
) {
    let enabled = {
        let config = state.config.config.read().expect("config lock");
        config.vrcx.enabled && config.vrcx.include_in_llm_context
    };
    if enabled {
        if let Some(entry) = state.integrations.vrcx.translation_context_entry() {
            context.push(entry);
        }
    }
}

pub(super) async fn translation_preview(
    State(state): State<ServiceContext>,
    Json(input): Json<PreviewInput>,
) -> ApiResult<Json<Value>> {
    let global = state.config.config.read().expect("config lock").clone();
    let config = state
        .config
        .language_session
        .read()
        .expect("language session lock")
        .apply_to(&global);
    let prompt_config = config.translation.prompt.clone();
    let mut context = db_call(Arc::clone(&state.content.db), move |db| {
        db.recent_translation_context(&prompt_config, None)
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "translation.context_load_failed",
            error.to_string(),
        )
    })?;
    append_vrcx_context(&state, &mut context);
    let mut target = config
        .translation
        .microphone_targets
        .first()
        .cloned()
        .ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                "translation.not_configured",
                "No microphone translation target is configured",
            )
        })?;
    if let Some(language) = &input.target_language {
        target.target_language = language.clone();
    }
    let result = state
        .content
        .translation_service
        .translate(
            &target,
            &config.translation.prompt,
            &config.asr.api_profiles,
            &input.text,
            input.source_language.as_deref(),
            &context,
        )
        .await
        .map_err(translation_error)?;
    Ok(Json(json!(result.into_record())))
}

pub(super) async fn subtitle_translate(
    State(state): State<ServiceContext>,
    Path(subtitle_id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let subtitle = db_call(Arc::clone(&state.content.db), move |db| {
        db.subtitle(subtitle_id)
    })
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
    let global = state.config.config.read().expect("config lock").clone();
    let config = state
        .config
        .language_session
        .read()
        .expect("language session lock")
        .apply_to(&global);
    if config.translation.mode == "disabled" {
        return Err(api_error(
            StatusCode::CONFLICT,
            "translation.disabled",
            "Translation is disabled",
        ));
    }
    let message_id = format!("translation-{}", uuid::Uuid::new_v4());
    let source = subtitle.source.clone();
    let prompt_config = config.translation.prompt.clone();
    let mut context = db_call(Arc::clone(&state.content.db), move |db| {
        db.recent_translation_context(&prompt_config, Some(subtitle_id))
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "translation.context_load_failed",
            error.to_string(),
        )
    })?;
    append_vrcx_context(&state, &mut context);
    let targets = if subtitle.source == "microphone" {
        &config.translation.microphone_targets
    } else {
        &config.translation.speaker_targets
    };
    if targets.is_empty() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "translation.not_configured",
            "No translation target is configured",
        ));
    }
    let mut records = Vec::with_capacity(targets.len());
    let mut last_error = None;
    for (index, target) in targets.iter().enumerate() {
        let preferred = index == 0;
        state
            .content
            .subtitle_output
            .translation_started_with_message(
                subtitle_id,
                &target.target_language,
                preferred,
                &message_id,
                &source,
            );
        let result = state
            .content
            .translation_service
            .translate(
                target,
                &config.translation.prompt,
                &config.asr.api_profiles,
                &subtitle.text,
                subtitle.language.as_deref(),
                &context,
            )
            .await;
        let record = match result {
            Ok(result) => result.into_record(),
            Err(error) => {
                state
                    .content
                    .subtitle_output
                    .translation_failed_with_message(TranslationFailure {
                        subtitle_id,
                        code: error.code.into(),
                        detail: error.detail.clone(),
                        target_language: &target.target_language,
                        preferred,
                        message_id: &message_id,
                        source: &source,
                    });
                last_error = Some(error);
                continue;
            }
        };
        let saved = record.clone();
        let conversation_catalog = state.content.conversation_catalog_tx.clone();
        if let Err(error) = db_call(Arc::clone(&state.content.db), move |db| {
            if db.save_translation(subtitle_id, &saved)? {
                publish_latest_catalog(db, &conversation_catalog);
            }
            Ok(())
        })
        .await
        {
            state
                .content
                .subtitle_output
                .translation_failed_with_message(TranslationFailure {
                    subtitle_id,
                    code: "translation.storage_failed".into(),
                    detail: error.to_string(),
                    target_language: &target.target_language,
                    preferred,
                    message_id: &message_id,
                    source: &source,
                });
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "translation.storage_failed",
                error.to_string(),
            ));
        }
        state
            .content
            .subtitle_output
            .translation_completed_with_message(
                subtitle_id,
                record.clone(),
                preferred,
                &message_id,
                &source,
            );
        records.push(record);
    }
    let record = records.into_iter().next().ok_or_else(|| {
        translation_error(last_error.unwrap_or_else(|| TranslationError {
            code: "translation.not_configured",
            detail: "No translation target is configured".into(),
            retryable: false,
        }))
    })?;
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
