use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::chatbox::{preview_chatbox, ChatboxComposeInput, ChatboxMessage, NewChatboxMessage};
use crate::db::conversations::publish_latest_catalog;
use crate::models::{now_iso8601, Subtitle, SubtitleTranslation};
use crate::osc::ManualSendError;

use super::{
    api_domain_error, api_error, api_error_with_params, db_call, ApiResult, OutputContext,
};

pub(super) async fn preview(Json(input): Json<ChatboxComposeInput>) -> ApiResult<Json<Value>> {
    let preview = preview_chatbox(&input).map_err(validation_error)?;
    Ok(Json(json!(preview)))
}

pub(super) async fn send(
    State(state): State<OutputContext>,
    Json(input): Json<ChatboxComposeInput>,
) -> ApiResult<Json<Value>> {
    send_input(&state, input).await
}

async fn send_input(state: &OutputContext, input: ChatboxComposeInput) -> ApiResult<Json<Value>> {
    let preview = preview_chatbox(&input).map_err(validation_error)?;
    if !preview.sendable {
        return Err(api_error_with_params(
            StatusCode::UNPROCESSABLE_ENTITY,
            "chatbox.over_limit",
            json!({ "count": preview.char_count, "limit": preview.limit }),
            "Chatbox message exceeds the character limit",
        ));
    }
    let message_id = format!("chatbox-{}", uuid::Uuid::new_v4());
    let sent = state
        .integrations
        .osc
        .send_manual(preview.text.clone())
        .await;
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
        Ok(_) => finish_delivery(state, saved, &message_id).await,
        Err(error) => {
            let _ = saved;
            Err(osc_error(error))
        }
    }
}

async fn record_delivery(
    state: &OutputContext,
    message: NewChatboxMessage,
) -> crate::error::AppResult<ChatboxMessage> {
    db_call(Arc::clone(&state.content.db), move |db| {
        db.add_chatbox_message(&message)
    })
    .await
}

async fn finish_delivery(
    state: &OutputContext,
    saved: crate::error::AppResult<ChatboxMessage>,
    message_id: &str,
) -> ApiResult<Json<Value>> {
    let message = saved.map_err(|error| api_domain_error(error, "chatbox.history_store_failed"))?;
    let subtitle = record_conversation_message(state, &message)
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
) -> crate::error::AppResult<Subtitle> {
    let subtitle = conversation_subtitle(message);
    let conversation_catalog = state.content.conversation_catalog_tx.clone();
    db_call(Arc::clone(&state.content.db), move |db| {
        let translations = subtitle.translations.clone();
        let mut saved = db.add_subtitle(&subtitle)?;
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

fn conversation_subtitle(message: &ChatboxMessage) -> Subtitle {
    let mut text = message.original.trim().to_owned();
    let mut language = message.source_language.clone();
    let mut translations = Vec::new();

    match message.send_mode.as_str() {
        "translation" => {
            if let Some(translation) = message.translation.as_deref() {
                text = translation.trim().to_owned();
                language = message.target_language.clone();
            }
        }
        "bilingual" => {
            if let Some(translation) = message.translation.as_deref() {
                translations.push(SubtitleTranslation {
                    text: translation.trim().to_owned(),
                    source_language: message.source_language.clone(),
                    target_language: message
                        .target_language
                        .clone()
                        .unwrap_or_else(|| "und".into()),
                    provider: "local".into(),
                    model: None,
                    created_at: message.created_at.clone(),
                });
            }
        }
        _ => {}
    }

    Subtitle {
        id: None,
        conversation_id: None,
        text,
        language,
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

fn osc_error(error: ManualSendError) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "osc.disabled" | "osc.blocked_vrchat_muted" | "osc.blocked_mute_unknown" => {
            StatusCode::CONFLICT
        }
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
        let bilingual = conversation_subtitle(&message("bilingual"));
        assert_eq!(bilingual.source, "chatbox");
        assert_eq!(bilingual.text, "hello");
        assert_eq!(bilingual.translations[0].text, "こんにちは");

        let translated = conversation_subtitle(&message("translation"));
        assert_eq!(translated.text, "こんにちは");
        assert_eq!(translated.language.as_deref(), Some("ja"));
        assert!(translated.translations.is_empty());
    }
}
