use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::chatbox::{
    CHATBOX_LIMIT, ChatboxComposeInput, ChatboxMessage, ChatboxMessageFormat, ChatboxOutputTarget,
    ChatboxPreferences, ChatboxSendMode, ChatboxTranslationOrigin, NewChatboxMessage, compact_text,
    preview_chatbox_with_limit,
};
use crate::db::conversations::publish_latest_catalog;
use crate::models::{Subtitle, SubtitleTranslation, now_iso8601};
use crate::osc::ManualSendError;

use super::{
    ApiResult, OutputContext, ServiceContext, api_domain_error, api_error, api_error_with_params,
    db_call,
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

pub(super) async fn get_preferences(State(state): State<OutputContext>) -> Json<Value> {
    let prefs = state
        .content
        .chatbox_preferences
        .read()
        .expect("chatbox preferences lock")
        .clone();
    Json(json!(prefs))
}

pub(super) async fn update_preferences(
    State(state): State<OutputContext>,
    Json(prefs): Json<ChatboxPreferences>,
) -> ApiResult<Json<Value>> {
    if prefs.target_language.chars().count() > crate::providers::MAX_TRANSLATION_LANGUAGE_LENGTH
        || prefs.target_language.chars().any(char::is_control)
    {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "chatbox.invalid_language",
            "Language code cannot contain control characters or exceed the maximum length",
        ));
    }
    *state
        .content
        .chatbox_preferences
        .write()
        .expect("chatbox preferences lock") = prefs.clone();
    Ok(Json(json!(prefs)))
}

pub(super) async fn send(
    State(state): State<OutputContext>,
    Json(input): Json<ChatboxComposeInput>,
) -> ApiResult<Json<Value>> {
    send_input(&state, input, "manual").await
}

pub(crate) fn osc_inbound_callback(state: ServiceContext) -> crate::osc::InboundChatboxCallback {
    Arc::new(move |text| {
        let state = state.clone();
        Box::pin(async move { process_osc_inbound_chatbox(&state, text).await })
    })
}

struct TypingIndicatorGuard {
    port: u16,
    active: bool,
}

impl TypingIndicatorGuard {
    fn new(port: u16) -> Self {
        Self { port, active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }

    async fn clear(&mut self) {
        if self.active {
            self.active = false;
            let _ = crate::osc::send_typing(self.port, false).await;
        }
    }
}

impl Drop for TypingIndicatorGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = crate::osc::send_typing_sync(self.port, false);
        }
    }
}

pub(crate) async fn process_osc_inbound_chatbox(
    state: &ServiceContext,
    text: String,
) -> Result<(), String> {
    let text = crate::chatbox::compact_text(&text);
    let compact_len = text.chars().count();
    if compact_len == 0 {
        return Err("Enter text before sending".into());
    }
    if compact_len > crate::chatbox::MAX_TEXT_LENGTH {
        return Err(format!(
            "Chatbox text must be at most {} characters",
            crate::chatbox::MAX_TEXT_LENGTH
        ));
    }

    let prefs = state
        .content
        .chatbox_preferences
        .read()
        .expect("chatbox preferences lock")
        .clone();
    let (osc_config, effective_target_language) = {
        let global = state.config.config.read().expect("config lock").clone();
        let effective = state
            .config
            .language_session
            .read()
            .expect("language session lock")
            .apply_to(&global);
        let target = if prefs.follow_translation_settings {
            effective
                .translation
                .microphone_targets
                .first()
                .map(|t| t.target_language.clone())
        } else {
            let trimmed = prefs.target_language.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        };
        (effective.osc, target)
    };

    let (send_osc, send_unitext) = resolve_outputs(
        state.integrations.osc.is_enabled(),
        state.integrations.unitext.is_enabled(),
        prefs.output_target,
    );
    if !send_osc && !send_unitext {
        return Err("Enable OSC or UniText in Settings before sending".into());
    }

    let mut typing_guard = None;
    let mut send_mode = prefs.send_mode;
    let mut translation = None;
    let mut translation_origin = None;
    let mut source_language = None;
    let mut target_language = effective_target_language.clone();

    if send_mode != ChatboxSendMode::Original {
        if send_osc && osc_config.port != osc_config.inbound.port {
            let _ = crate::osc::send_typing(osc_config.port, true).await;
            typing_guard = Some(TypingIndicatorGuard::new(osc_config.port));
        }
        match super::translation::translate_chatbox_text(
            state,
            &text,
            None,
            effective_target_language.as_deref(),
        )
        .await
        {
            Ok(result) => {
                translation = Some(result.text);
                translation_origin = Some(ChatboxTranslationOrigin {
                    provider: result.provider,
                    model: result.model,
                });
                source_language = result.source_language;
                target_language = Some(result.target_language);
            }
            Err(error) => {
                tracing::warn!(
                    code = error.code,
                    detail = %error.detail,
                    "OSC inbound Chatbox translation failed; falling back to original text"
                );
                send_mode = ChatboxSendMode::Original;
            }
        }
    }

    let input = ChatboxComposeInput {
        original: text,
        translation,
        translation_origin,
        source_language,
        target_language,
        send_mode,
        output_target: prefs.output_target,
        message_format: osc_config.message_format,
        custom_format: (osc_config.message_format == ChatboxMessageFormat::Custom)
            .then_some(osc_config.custom_format)
            .flatten(),
        overflow_policy: prefs.overflow_policy,
    };
    let output_ctx = OutputContext {
        content: Arc::clone(&state.content),
        integrations: Arc::clone(&state.integrations),
    };
    let (result, osc_sent) = send_input_with_outcome(&output_ctx, input, "osc_inbound").await;
    if let Some(guard) = typing_guard.as_mut() {
        if osc_sent {
            guard.disarm();
        } else {
            guard.clear().await;
        }
    }
    result.map(|_| ()).map_err(|(_, body)| {
        body.get("detail")
            .and_then(Value::as_str)
            .unwrap_or("OSC inbound Chatbox send failed")
            .to_owned()
    })
}

async fn send_input(
    state: &OutputContext,
    input: ChatboxComposeInput,
    source: &str,
) -> ApiResult<Json<Value>> {
    send_input_with_outcome(state, input, source).await.0
}

async fn send_input_with_outcome(
    state: &OutputContext,
    input: ChatboxComposeInput,
    source: &str,
) -> (ApiResult<Json<Value>>, bool) {
    let (send_osc, send_unitext) = resolve_outputs(
        state.integrations.osc.is_enabled(),
        state.integrations.unitext.is_enabled(),
        input.output_target,
    );
    let enforce_osc_limit = send_osc || !send_unitext;
    let preview = match preview_chatbox_with_limit(&input, enforce_osc_limit) {
        Ok(preview) => preview,
        Err(error) => return (Err(validation_error(error)), false),
    };
    if !preview.sendable {
        return (
            Err(api_error_with_params(
                StatusCode::UNPROCESSABLE_ENTITY,
                "chatbox.over_limit",
                json!({
                    "count": preview.char_count,
                    "limit": preview.limit.unwrap_or(CHATBOX_LIMIT)
                }),
                "Chatbox message exceeds the character limit",
            )),
            false,
        );
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
    let mut osc_sent = false;
    let mut partial_error: Option<ManualSendError> = None;
    let sent = match (send_osc, send_unitext) {
        (false, false) => Err(ManualSendError {
            code: "chatbox.output_disabled",
            detail: "Enable OSC or UniText in Settings before sending".into(),
        }),
        (true, false) => {
            let res = state
                .integrations
                .osc
                .send_manual(preview.text.clone())
                .await;
            osc_sent = res.is_ok();
            res
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
            osc_sent = osc_res.is_ok();
            match (osc_res, unitext_res) {
                (Ok(sent_at), Ok(_)) => Ok(sent_at),
                (Ok(sent_at), Err(error)) => {
                    tracing::warn!(
                        code = error.code,
                        detail = %error.detail,
                        "UniText manual Chatbox send failed while OSC succeeded"
                    );
                    partial_error = Some(error);
                    Ok(sent_at)
                }
                (Err(error), Ok(sent_at)) => {
                    tracing::warn!(
                        code = error.code,
                        detail = %error.detail,
                        "OSC manual Chatbox send failed while UniText succeeded"
                    );
                    partial_error = Some(error);
                    Ok(sent_at)
                }
                (Err(osc_err), Err(_)) => Err(osc_err),
            }
        }
    };
    let translation_origin = input.translation_origin.clone();
    let recorded_error = sent.as_ref().err().or(partial_error.as_ref());
    let record = NewChatboxMessage {
        source: source.into(),
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
        error_code: recorded_error.map(|error| error.code.into()),
        error_detail: recorded_error.map(|error| error.detail.clone()),
        resent_from_id: None,
        created_at: now_iso8601(),
        sent_at: sent.as_ref().ok().cloned(),
    };
    let saved = record_delivery(state, record).await;
    let result = match sent {
        Ok(_) => finish_delivery(state, saved, &message_id, translation_origin).await,
        Err(error) => {
            let _ = saved;
            Err(send_error(error))
        }
    };
    (result, osc_sent)
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

    #[tokio::test]
    async fn osc_inbound_chatbox_syncs_preferences_translates_sends_typing_and_persists() {
        use axum::{Router, routing::post};
        use std::time::Duration;
        use tokio::net::UdpSocket;

        let router = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                Json(json!({
                    "choices": [{"message": {"content": "こんにちは"}}]
                }))
            }),
        );
        let llm_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let llm_url = format!("http://{}/v1", llm_listener.local_addr().unwrap());
        let llm_server =
            tokio::spawn(async move { axum::serve(llm_listener, router).await.unwrap() });

        let vrchat_rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let vrchat_port = vrchat_rx.local_addr().unwrap().port();
        let free_probe = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let inbound_port = free_probe.local_addr().unwrap().port();
        drop(free_probe);

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let mut config = crate::config::AppConfig::default();
        config.storage.database_path = dir.path().join("vrcs.db").to_string_lossy().into_owned();
        config.audio.output.mode = "disabled".into();
        config.audio.microphone.mode = "disabled".into();
        config.osc.enabled = true;
        config.osc.port = vrchat_port;
        config.osc.mute_sync_enabled = false;
        config.osc.inbound.enabled = true;
        config.osc.inbound.port = inbound_port;
        config.asr.api_profiles = vec![crate::config::ApiProfile {
            id: "mock-llm".into(),
            name: "Mock LLM".into(),
            provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(llm_url),
            auth_mode: crate::config::ApiAuthMode::None,
            is_local: true,
            enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
            ..Default::default()
        }];
        config.translation.microphone_targets[0].use_default_model = false;
        config.translation.microphone_targets[0].profile_id = Some("mock-llm".into());
        config.translation.microphone_targets[0].model = "test-model".into();
        crate::config::save_config(&config_path, &config).unwrap();

        let handle = crate::start(crate::CoreOptions {
            config_path,
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: Some("test-token".into()),
            vad_model_path: None,
            asr_model_dir: None,
        })
        .await
        .unwrap();

        let client = reqwest::Client::new();
        let base = format!("http://{}", handle.address());
        client
            .put(format!("{base}/api/chatbox/preferences"))
            .bearer_auth("test-token")
            .json(&json!({
                "follow_translation_settings": false,
                "target_language": "ja",
                "send_mode": "bilingual",
                "output_target": "osc",
                "overflow_policy": "smart_truncate"
            }))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let packet = rosc::encoder::encode(&rosc::OscPacket::Message(rosc::OscMessage {
            addr: "/chatbox/input".into(),
            args: vec![
                rosc::OscType::String("hello".into()),
                rosc::OscType::Bool(true),
            ],
        }))
        .unwrap();
        sender
            .send_to(&packet, ("127.0.0.1", inbound_port))
            .await
            .unwrap();

        let mut saw_typing = false;
        let mut saw_chatbox = None;
        let mut buf = [0u8; 1024];
        for _ in 0..4 {
            let Ok(Ok((len, _))) =
                tokio::time::timeout(Duration::from_secs(3), vrchat_rx.recv_from(&mut buf)).await
            else {
                break;
            };
            let (_, pkt) = rosc::decoder::decode_udp(&buf[..len]).unwrap();
            if let rosc::OscPacket::Message(msg) = pkt {
                if msg.addr == "/chatbox/typing" && msg.args == vec![rosc::OscType::Bool(true)] {
                    saw_typing = true;
                } else if msg.addr == "/chatbox/input"
                    && let Some(rosc::OscType::String(text)) = msg.args.first()
                {
                    saw_chatbox = Some(text.clone());
                    break;
                }
            }
        }
        assert!(
            saw_typing,
            "expected /chatbox/typing true during translation"
        );
        assert_eq!(saw_chatbox.as_deref(), Some("hello\nこんにちは"));

        let health: Value = client
            .get(format!("{base}/health"))
            .bearer_auth("test-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(health["osc_inbound"]["state"], "listening");
        assert_eq!(health["osc_inbound"]["intercepted_chatbox_messages"], 1);

        handle.shutdown().await.unwrap();
        llm_server.abort();
    }
}
