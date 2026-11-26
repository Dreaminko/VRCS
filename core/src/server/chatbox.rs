use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::chatbox::{
    CHATBOX_LIMIT, ChatboxComposeInput, ChatboxMessage, ChatboxOutputTarget, ChatboxSendMode,
    ChatboxTranslationOrigin, NewChatboxMessage, compact_text, preview_chatbox_with_limit,
};
use crate::db::conversations::publish_latest_catalog;
use crate::models::{Subtitle, SubtitleTranslation, now_iso8601};
use crate::osc::ManualSendError;

use super::{
    ApiResult, OutputContext, api_domain_error, api_error, api_error_with_params, db_call,
};

fn resolve_outputs(
    osc_enabled: bool,
    unitext_enabled: bool,
    target: ChatboxOutputTarget,
) -> (bool, bool) {
    match (osc_enabled, unitext_enabled) {
        (true, true) => match target {
            ChatboxOutputTarget::Osc => (true, false),
            ChatboxOutputTarget::Unitext => (false, true),
            ChatboxOutputTarget::Both => (true, true),
        },
        other => other,
    }
}

pub(super) async fn preview(
    State(state): State<OutputContext>,
    Json(input): Json<ChatboxComposeInput>,
) -> ApiResult<Json<Value>> {
    let (send_osc, send_unitext) = resolve_outputs(
        state.integrations.osc.is_enabled(),
        state.integrations.unitext.is_enabled(),
        input.output_target,
    );
    let enforce_osc_limit = send_osc || !send_unitext;
    let preview =
        preview_chatbox_with_limit(&input, enforce_osc_limit).map_err(validation_error)?;
    Ok(Json(json!(preview)))
}

pub(super) async fn send(
    State(state): State<OutputContext>,
    Json(input): Json<ChatboxComposeInput>,
) -> ApiResult<Json<Value>> {
    send_input(&state, input).await
}

async fn send_input(state: &OutputContext, input: ChatboxComposeInput) -> ApiResult<Json<Value>> {
    let (send_osc, send_unitext) = resolve_outputs(
        state.integrations.osc.is_enabled(),
        state.integrations.unitext.is_enabled(),
        input.output_target,
    );
    let enforce_osc_limit = send_osc || !send_unitext;
    let preview =
        preview_chatbox_with_limit(&input, enforce_osc_limit).map_err(validation_error)?;
    if !preview.sendable {
        return Err(api_error_with_params(
            StatusCode::UNPROCESSABLE_ENTITY,
            "chatbox.over_limit",
            json!({
                "count": preview.char_count,
                "limit": preview.limit.unwrap_or(CHATBOX_LIMIT)
            }),
            "Chatbox message exceeds the character limit",
        ));
    }
    let message_id = format!("chatbox-{}", uuid::Uuid::new_v4());
    let unitext_payload = if send_unitext {
        let compact_original = compact_text(&input.original);
        let compact_translation = input.translation.as_deref().map(compact_text);
        match input.send_mode {
            ChatboxSendMode::Original => (compact_original, None, None),
            ChatboxSendMode::Translation => (String::new(), compact_translation, Some(false)),
            ChatboxSendMode::Bilingual => (compact_original, compact_translation, Some(true)),
        }
    } else {
        (String::new(), None, None)
    };
    let sent = match (send_osc, send_unitext) {
        (false, false) => Err(ManualSendError {
            code: "chatbox.output_disabled",
            detail: "Enable OSC or UniText in Settings before sending".into(),
        }),
        (true, false) => {
            state
                .integrations
                .osc
                .send_manual(preview.text.clone())
                .await
        }
        (false, true) => {
            state
                .integrations
                .unitext
                .send_manual(
                    message_id.clone(),
                    unitext_payload.0,
                    unitext_payload.1,
                    unitext_payload.2,
                )
                .await
        }
        (true, true) => {
            let (osc_res, unitext_res) = tokio::join!(
                state.integrations.osc.send_manual(preview.text.clone()),
                state.integrations.unitext.send_manual(
                    message_id.clone(),
                    unitext_payload.0,
                    unitext_payload.1,
                    unitext_payload.2,
                ),
            );
            match (osc_res, unitext_res) {
                (Ok(sent_at), Ok(_)) => Ok(sent_at),
                (Ok(sent_at), Err(error)) => {
                    tracing::warn!(
                        code = error.code,
                        detail = %error.detail,
                        "UniText manual Chatbox send failed while OSC succeeded"
                    );
                    Ok(sent_at)
                }
                (Err(error), Ok(sent_at)) => {
                    tracing::warn!(
                        code = error.code,
                        detail = %error.detail,
                        "OSC manual Chatbox send failed while UniText succeeded"
                    );
                    Ok(sent_at)
                }
                (Err(osc_err), Err(_)) => Err(osc_err),
            }
        }
    };
    let translation_origin = input.translation_origin.clone();
    let record = NewChatboxMessage {
        source: "manual".into(),
        original: input.original,
        translation: input.translation,
        source_language: input.source_language,
        target_language: input.target_language,
        send_mode: input.send_mode.as_str().into(),
        message_format: input.message_format.as_str().into(),
        custom_format: input.custom_format,
        rendered_text: preview.text,
        char_count: preview.char_count,
        truncated: preview.truncated,
        status: if sent.is_ok() { "sent" } else { "failed" }.into(),
        error_code: sent.as_ref().err().map(|error| error.code.into()),
        error_detail: sent.as_ref().err().map(|error| error.detail.clone()),
        resent_from_id: None,
        created_at: now_iso8601(),
        sent_at: sent.as_ref().ok().cloned(),
    };
    let saved = record_delivery(state, record).await;
    match sent {
        Ok(_) => finish_delivery(state, saved, &message_id, translation_origin).await,
        Err(error) => {
            let _ = saved;
            Err(send_error(error))
        }
    }
}

async fn record_delivery(
    state: &OutputContext,
    message: NewChatboxMessage,
) -> crate::error::AppResult<ChatboxMessage> {
    let context_scope_id = state.content.room_context.snapshot().scope_id;
    db_call(Arc::clone(&state.content.db), move |db| {
        db.add_chatbox_message_in_scope(&message, context_scope_id.as_deref(), None)
    })
    .await
}

async fn finish_delivery(
    state: &OutputContext,
    saved: crate::error::AppResult<ChatboxMessage>,
    message_id: &str,
    translation_origin: Option<ChatboxTranslationOrigin>,
) -> ApiResult<Json<Value>> {
    let message = saved.map_err(|error| api_domain_error(error, "chatbox.history_store_failed"))?;
    let subtitle = record_conversation_message(state, &message, translation_origin.as_ref())
        .await
        .map_err(|error| api_domain_error(error, "chatbox.conversation_store_failed"))?;
    state.content.subtitle_output.subtitle_recorded(subtitle);
    state
        .content
        .subtitle_output
        .chatbox_sent(message_id, &message);
    Ok(Json(json!(message)))
}

async fn record_conversation_message(
    state: &OutputContext,
    message: &ChatboxMessage,
    translation_origin: Option<&ChatboxTranslationOrigin>,
) -> crate::error::AppResult<Subtitle> {
    let subtitle = conversation_subtitle(message, translation_origin);
    let context_scope_id = state.content.room_context.snapshot().scope_id;
    let conversation_catalog = state.content.conversation_catalog_tx.clone();
    db_call(Arc::clone(&state.content.db), move |db| {
        let translations = subtitle.translations.clone();
        let mut saved = db.add_subtitle_in_scope(&subtitle, context_scope_id.as_deref(), None)?;
        if let Some(subtitle_id) = saved.id {
            for translation in translations {
                db.save_translation(subtitle_id, &translation)?;
                saved.translations.push(translation);
            }
        }
        publish_latest_catalog(db, &conversation_catalog);
        Ok(saved)
    })
    .await
}

fn conversation_subtitle(
    message: &ChatboxMessage,
    translation_origin: Option<&ChatboxTranslationOrigin>,
) -> Subtitle {
    let mut translations = Vec::new();

    if matches!(message.send_mode.as_str(), "translation" | "bilingual")
        && let Some(translation) = message.translation.as_deref()
    {
        translations.push(SubtitleTranslation {
            source_group: None,
            text: translation.trim().to_owned(),
            source_language: message.source_language.clone(),
            target_language: message
                .target_language
                .clone()
                .unwrap_or_else(|| "und".into()),
            provider: translation_origin
                .map(|origin| origin.provider.clone())
                .unwrap_or_else(|| "local".into()),
            model: translation_origin.and_then(|origin| origin.model.clone()),
            created_at: message.created_at.clone(),
        });
    }

    Subtitle {
        speaker: None,
        id: None,
        conversation_id: None,
        text: message.original.trim().to_owned(),
        language: message.source_language.clone(),
        started_at: None,
        ended_at: None,
        source: "chatbox".into(),
        created_at: message.created_at.clone(),
        translations,
    }
}

fn validation_error(error: crate::chatbox::ChatboxValidationError) -> (StatusCode, Json<Value>) {
    api_error(StatusCode::UNPROCESSABLE_ENTITY, error.code, error.detail)
}

fn send_error(error: ManualSendError) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "chatbox.output_disabled"
        | "osc.disabled"
        | "osc.blocked_vrchat_muted"
        | "osc.blocked_mute_unknown"
        | "unitext.disabled"
        | "unitext.not_detected" => StatusCode::CONFLICT,
        "osc.send_timeout" => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    api_error(status, error.code, error.detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(send_mode: &str) -> ChatboxMessage {
        ChatboxMessage {
            id: 1,
            source: "manual".into(),
            original: "hello".into(),
            translation: Some("こんにちは".into()),
            source_language: Some("en".into()),
            target_language: Some("ja".into()),
            send_mode: send_mode.into(),
            message_format: "original_newline_translation".into(),
            custom_format: None,
            rendered_text: "hello\nこんにちは".into(),
            char_count: 11,
            truncated: false,
            status: "sent".into(),
            error_code: None,
            error_detail: None,
            resent_from_id: None,
            created_at: "2026-08-14T00:00:00Z".into(),
            sent_at: Some("2026-08-14T00:00:00Z".into()),
        }
    }

    #[test]
    fn sent_chatbox_messages_become_conversation_subtitles() {
        let local = ChatboxTranslationOrigin {
            provider: "plamo_local".into(),
            model: Some("plamo-3-translate-0.8b-q4_k_m".into()),
        };
        let bilingual = conversation_subtitle(&message("bilingual"), Some(&local));
        assert_eq!(bilingual.source, "chatbox");
        assert_eq!(bilingual.text, "hello");
        assert_eq!(bilingual.translations[0].text, "こんにちは");
        assert_eq!(bilingual.translations[0].provider, "plamo_local");
        assert_eq!(
            bilingual.translations[0].model.as_deref(),
            Some("plamo-3-translate-0.8b-q4_k_m")
        );

        let translated = conversation_subtitle(&message("translation"), Some(&local));
        assert_eq!(translated.text, "hello");
        assert_eq!(translated.language.as_deref(), Some("en"));
        assert_eq!(translated.translations[0].text, "こんにちは");
        assert_eq!(translated.translations[0].provider, "plamo_local");
        assert_eq!(
            translated.translations[0].model.as_deref(),
            Some("plamo-3-translate-0.8b-q4_k_m")
        );
    }
}
