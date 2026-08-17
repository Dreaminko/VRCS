use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::conversations::publish_latest_catalog;
use crate::translation::{GlossarySubscriptionError, TranslationError};
use crate::{
    config::{validate_translation_prompt, TranslationPromptConfig},
    translation::TranslationPromptBuilder,
};

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
    State(state): State<Arc<AppState>>,
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
    let mut context = db_call(Arc::clone(&state.db), move |db| {
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
    let prompt = state.glossary_subscription.merged_prompt(&input.prompt);
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

pub(super) async fn glossary_statuses(State(state): State<Arc<AppState>>) -> Json<Value> {
    let prompt = current_prompt(&state);
    Json(json!(state.glossary_subscription.statuses(&prompt)))
}

pub(super) async fn glossary_refresh(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .glossary_subscription
        .refresh(&id)
        .await
        .map_err(glossary_subscription_error)?;
    let prompt = current_prompt(&state);
    Ok(Json(json!(state.glossary_subscription.statuses(&prompt))))
}

pub(super) async fn glossary_subscription_status(
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    let prompt = current_prompt(&state);
    Json(json!(state.glossary_subscription.legacy_status(&prompt)))
}

pub(super) async fn glossary_subscription_refresh(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Value>> {
    let id = state
        .glossary_subscription
        .configured_ids()
        .into_iter()
        .next()
        .ok_or_else(|| {
            glossary_subscription_error(GlossarySubscriptionError {
                code: "glossary_subscription.not_found",
                detail: "No glossary subscription is configured".into(),
            })
        })?;
    state
        .glossary_subscription
        .refresh(&id)
        .await
        .map_err(glossary_subscription_error)?;
    let prompt = current_prompt(&state);
    Ok(Json(json!(state
        .glossary_subscription
        .legacy_status(&prompt))))
}

fn current_prompt(state: &AppState) -> TranslationPromptConfig {
    state
        .config
        .read()
        .expect("config lock")
        .translation
        .prompt
        .clone()
}

fn append_vrcx_context(
    state: &AppState,
    context: &mut Vec<crate::translation::TranslationContextEntry>,
) {
    let enabled = {
        let config = state.config.read().expect("config lock");
        config.vrcx.enabled && config.vrcx.include_in_llm_context
    };
    if enabled {
        if let Some(entry) = state.vrcx.translation_context_entry() {
            context.push(entry);
        }
    }
}

pub(super) async fn translation_preview(
    State(state): State<Arc<AppState>>,
    Json(input): Json<PreviewInput>,
) -> ApiResult<Json<Value>> {
    let config = state.config.read().expect("config lock").clone();
    let prompt_config = config.translation.prompt.clone();
    let mut context = db_call(Arc::clone(&state.db), move |db| {
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
    let result = state
        .translation_service
        .translate(
            &config.translation,
            &config.asr.api_profiles,
            &input.text,
            input.source_language.as_deref(),
            input.target_language.as_deref(),
            &context,
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
    let message_id = format!("translation-{}", uuid::Uuid::new_v4());
    let source = subtitle.source.clone();
    let prompt_config = config.translation.prompt.clone();
    let mut context = db_call(Arc::clone(&state.db), move |db| {
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
    state
        .subtitle_output
        .translation_started_with_message(subtitle_id, &message_id, &source);
    let result = state
        .translation_service
        .translate(
            &config.translation,
            &config.asr.api_profiles,
            &subtitle.text,
            subtitle.language.as_deref(),
            None,
            &context,
        )
        .await;
    let record = match result {
        Ok(result) => result.into_record(),
        Err(error) => {
            state.subtitle_output.translation_failed_with_message(
                subtitle_id,
                error.code.into(),
                error.detail.clone(),
                &message_id,
                &source,
            );
            return Err(translation_error(error));
        }
    };
    let saved = record.clone();
    let conversation_catalog = state.conversation_catalog_tx.clone();
    if let Err(error) = db_call(Arc::clone(&state.db), move |db| {
        if db.save_translation(subtitle_id, &saved)? {
            publish_latest_catalog(db, &conversation_catalog);
        }
        Ok(())
    })
    .await
    {
        state.subtitle_output.translation_failed_with_message(
            subtitle_id,
            "translation.storage_failed".into(),
            error.to_string(),
            &message_id,
            &source,
        );
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "translation.storage_failed",
            error.to_string(),
        ));
    }
    state.subtitle_output.translation_completed_with_message(
        subtitle_id,
        record.clone(),
        &message_id,
        &source,
    );
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

fn glossary_subscription_error(error: GlossarySubscriptionError) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "glossary_subscription.not_found" => StatusCode::NOT_FOUND,
        "glossary_subscription.disabled" => StatusCode::CONFLICT,
        "glossary_subscription.invalid_url"
        | "glossary_subscription.invalid_redirect"
        | "glossary_subscription.invalid_json"
        | "glossary_subscription.unsupported_version"
        | "glossary_subscription.too_large"
        | "glossary_subscription.too_many_entries"
        | "glossary_subscription.invalid_entry"
        | "glossary_subscription.duplicate_entry" => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::BAD_GATEWAY,
    };
    api_error(status, error.code, error.detail)
}
