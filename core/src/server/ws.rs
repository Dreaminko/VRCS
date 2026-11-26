use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};

use crate::domain_events::DomainEvent;
use crate::models::LiveTranscription;

use super::{ALLOWED_ORIGINS, RealtimeContext, api_error, token_eq};

const WS_OUTBOUND_CAPACITY: usize = 256;
const WS_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN_PROTOCOL_PREFIX: &str = "vrcs.token.";

pub(super) async fn ws_handler(
    State(state): State<RealtimeContext>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let expected = &state.integrations.session_token;
    let bearer_ok = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| token_eq(value, expected));
    let matched_protocol = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .find(|protocol| {
            protocol
                .strip_prefix(TOKEN_PROTOCOL_PREFIX)
                .is_some_and(|value| token_eq(value, expected))
        })
        .map(str::to_owned);
    let query_ok = params
        .get("token")
        .map(String::as_str)
        .is_some_and(|value| token_eq(value, expected));

    if !bearer_ok && matched_protocol.is_none() && !query_ok {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "auth.unauthorized",
            "Unauthorized",
        )
        .into_response();
    }
    if headers.get(header::ORIGIN).is_some_and(|origin| {
        origin
            .to_str()
            .map_or(true, |origin| !ALLOWED_ORIGINS.contains(&origin))
    }) {
        return api_error(StatusCode::FORBIDDEN, "auth.origin_forbidden", "Forbidden")
            .into_response();
    }
    let ws = if let Some(protocol) = matched_protocol {
        ws.protocols([Cow::Owned(protocol)])
    } else {
        ws
    };
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

fn enqueue_text(
    outbound_tx: &mpsc::Sender<Message>,
    payload: impl Into<String>,
    drop_if_full: bool,
) -> bool {
    match outbound_tx.try_send(Message::Text(payload.into().into())) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => drop_if_full,
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}

pub(super) async fn handle_socket(state: RealtimeContext, socket: WebSocket) {
    let mut receiver = state.content.subtitle_output.subscribe_subtitles();
    let mut recognition_receiver = state.integrations.domain_events.subscribe_ui();
    let mut live_receiver = state.capture.live_tx.subscribe();
    let mut catalog_receiver = state.content.conversation_catalog_tx.subscribe();
    let mut translation_receiver = state.content.subtitle_output.subscribe_translations();
    let mut mute_receiver = state.integrations.vrchat_mute_sync.subscribe();
    let mut shutdown = state.integrations.shutdown.clone();
    let (mut sender, mut incoming) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Message>(WS_OUTBOUND_CAPACITY);
    let writer_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            match tokio::time::timeout(WS_WRITE_TIMEOUT, sender.send(message)).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => return,
            }
        }
        let _ = tokio::time::timeout(WS_WRITE_TIMEOUT, sender.send(Message::Close(None))).await;
    });

    if !enqueue_text(&outbound_tx, r#"{"type":"connected"}"#, false) {
        writer_task.abort();
        return;
    }
    match super::db_call(Arc::clone(&state.content.db), |db| {
        db.conversation_catalog()
    })
    .await
    {
        Ok(catalog) => {
            let payload = json!({
                "type": "conversation_catalog",
                "catalog": catalog,
            })
            .to_string();
            if !enqueue_text(&outbound_tx, payload, false) {
                writer_task.abort();
                return;
            }
        }
        Err(error) => tracing::warn!(%error, "initial conversation catalog could not be sent"),
    }
    let mut kicked_slow_client = false;
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                break;
            }
            message = incoming.next() => {
                match message {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            subtitle = receiver.recv() => {
                match subtitle {
                    Ok(subtitle) => {
                        if matches!(subtitle.source.as_str(), "speaker" | "microphone") {
                            continue;
                        }
                        let payload = json!({ "type": "subtitle", "subtitle": subtitle }).to_string();
                        if !enqueue_text(&outbound_tx, payload, false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::warn!(dropped, "subtitle broadcast lagged; requesting client resync");
                        if !enqueue_text(&outbound_tx, r#"{"type":"resync_required"}"#, false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            event = recognition_receiver.recv() => {
                match event {
                    Ok(event) => {
                        let Some(payload) = recognition_payload(event) else {
                            continue;
                        };
                        if !enqueue_text(&outbound_tx, payload.to_string(), false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::warn!(dropped, "recognition event stream lagged; resetting client state");
                        let payload = json!({ "type": "recognition_reset" }).to_string();
                        if !enqueue_text(&outbound_tx, payload, false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            event = live_receiver.recv() => {
                match event {
                    Ok(LiveTranscription::AudioLevel { source, rms_dbfs, peak_dbfs, speech }) => {
                        let payload = json!({
                            "type": "audio_level",
                            "source": source,
                            "rms_dbfs": rms_dbfs,
                            "peak_dbfs": peak_dbfs,
                            "speech": speech,
                        });
                        if !enqueue_text(&outbound_tx, payload.to_string(), true) {
                            break;
                        }
                    }
                    Ok(LiveTranscription::Partial { .. } | LiveTranscription::Failed { .. }) => continue,
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::warn!(dropped, "live stream lagged; requesting client resync");
                        if !enqueue_text(&outbound_tx, r#"{"type":"resync_required"}"#, false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            catalog = catalog_receiver.recv() => {
                match catalog {
                    Ok(catalog) => {
                        let payload = json!({
                            "type": "conversation_catalog",
                            "catalog": catalog,
                        }).to_string();
                        if !enqueue_text(&outbound_tx, payload, false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::warn!(dropped, "conversation catalog stream lagged; requesting client resync");
                        if !enqueue_text(&outbound_tx, r#"{"type":"resync_required"}"#, false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            event = translation_receiver.recv() => {
                match event {
                    Ok(event) => {
                        let payload = serde_json::to_string(&event).expect("translation event serialization");
                        if !enqueue_text(&outbound_tx, payload, false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::warn!(dropped, "translation stream lagged; requesting client resync");
                        if !enqueue_text(&outbound_tx, r#"{"type":"resync_required"}"#, false) {
                            kicked_slow_client = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            changed = mute_receiver.changed() => {
                if changed.is_err() {
                    break;
                }
                let payload = json!({
                    "type": "vrchat_mute_status",
                    "status": mute_receiver.borrow().clone(),
                }).to_string();
                if !enqueue_text(&outbound_tx, payload, false) {
                    kicked_slow_client = true;
                    break;
                }
            }
        }
    }
    drop(outbound_tx);
    if kicked_slow_client {
        writer_task.abort();
    } else {
        let _ = writer_task.await;
    }
}

fn recognition_payload(event: DomainEvent) -> Option<Value> {
    match event.event_type.as_str() {
        "asr.speaker_updated" => {
            Some(json!({ "type": "speaker_updated", "speaker": event.payload["speaker"] }))
        }
        "translation.live_updated" => {
            let mut payload = event.payload;
            payload["type"] = json!("live_translation_updated");
            payload["source"] = json!(event.source);
            Some(payload)
        }
        "asr.partial" => {
            let mut payload = json!({
            "type": "partial",
            "utterance_id": event.message_id,
            "source": event.source,
            "text": event.payload.get("text")?,
            "language": event.payload.get("language"),
            });
            if let Some(speaker) = event.payload.get("speaker") {
                payload["speaker"] = speaker.clone();
            }
            Some(payload)
        }
        "asr.final" => Some(json!({
            "type": "subtitle",
            "utterance_id": event.message_id,
            "subtitle": event.payload.get("subtitle")?,
        })),
        "asr.cancelled" => Some(json!({
            "type": "recognition_cancelled",
            "utterance_id": event.message_id,
            "source": event.source,
            "reason": event.payload.get("reason")?,
        })),
        "asr.reset" => Some(json!({
            "type": "recognition_reset",
            "source": event.source,
        })),
        "asr.failed" => Some(json!({
            "type": "failed",
            "utterance_id": event.payload.get("utterance_id"),
            "source": event.source,
            "code": event.payload.get("code"),
            "detail": event.payload.get("detail"),
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain_events::DomainEventHub;
    use crate::models::{Subtitle, now_iso8601};

    #[tokio::test]
    async fn player_partial_keeps_identity_over_the_ui_websocket() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        let speaker = crate::models::SpeakerIdentity {
            id: "session:flow:actor".into(),
            index: 2,
            display_name: None,
            user_id: None,
        };
        events.asr_player_partial("player-scoped:1", "speaker", "hello", Some("en"), &speaker);
        let payload = recognition_payload(receiver.recv().await.unwrap()).unwrap();
        assert_eq!(payload["speaker"], serde_json::json!(speaker));
        assert_eq!(payload["utterance_id"], "player-scoped:1");
    }

    fn subtitle() -> Subtitle {
        Subtitle {
            speaker: None,
            id: Some(7),
            conversation_id: Some("conversation-test".into()),
            text: "hello world".into(),
            language: Some("en".into()),
            started_at: None,
            ended_at: None,
            source: "speaker".into(),
            created_at: now_iso8601(),
            translations: Vec::new(),
        }
    }

    #[tokio::test]
    async fn live_translation_is_a_display_event_without_a_subtitle_id() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        events.live_translation(
            "speaker",
            &crate::models::LiveTranslation {
                source_utterance_id: Some("source-1".into()),
                completed_original: Some("complete original".into()),
                conversation_preview: Some(crate::models::LiveTranslationPreview {
                    text: "original".into(),
                    translation: "full translation".into(),
                }),
                speaker: None,
                utterance_id: "live-1".into(),
                text: String::new(),
                language: None,
                translation: "hello".into(),
                target_language: "en".into(),
            },
        );
        let payload = recognition_payload(receiver.recv().await.unwrap()).unwrap();
        assert_eq!(payload["type"], "live_translation_updated");
        assert_eq!(payload["translation"], "hello");
        assert_eq!(payload["source"], "speaker");
        assert_eq!(payload["source_utterance_id"], "source-1");
        assert_eq!(payload["completed_original"], "complete original");
        assert_eq!(
            payload["conversation_preview"]["translation"],
            "full translation"
        );
        assert!(payload.get("subtitle_id").is_none());
    }

    #[tokio::test]
    async fn cancelled_and_failed_events_preserve_utterance_identity() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        events.asr_cancelled("utterance-7", "speaker", "filtered");
        events.asr_failed(
            Some("utterance-8"),
            "microphone",
            "asr.cloud_error",
            "failed",
        );

        let cancelled = recognition_payload(receiver.recv().await.unwrap()).unwrap();
        let failed = recognition_payload(receiver.recv().await.unwrap()).unwrap();

        assert_eq!(cancelled["type"], "recognition_cancelled");
        assert_eq!(cancelled["utterance_id"], "utterance-7");
        assert_eq!(cancelled["reason"], "filtered");
        assert_eq!(failed["type"], "failed");
        assert_eq!(failed["utterance_id"], "utterance-8");
        assert_eq!(failed["source"], "microphone");
    }

    #[tokio::test]
    async fn recognition_resets_are_scoped_to_their_source() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        events.asr_reset("speaker");

        let reset = recognition_payload(receiver.recv().await.unwrap()).unwrap();
        assert_eq!(reset["type"], "recognition_reset");
        assert_eq!(reset["source"], "speaker");
    }

    #[tokio::test]
    async fn session_failures_have_no_utterance_identity() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        events.asr_failed(None, "speaker", "asr.cloud_disconnected", "closed");

        let failed = recognition_payload(receiver.recv().await.unwrap()).unwrap();
        assert_eq!(failed["type"], "failed");
        assert!(failed["utterance_id"].is_null());
    }

    #[tokio::test]
    async fn recognition_events_preserve_utterance_order_and_identity() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        events.asr_partial("utterance-7", "speaker", "hello", Some("en"));
        events.asr_final("utterance-7", &subtitle());

        let partial = recognition_payload(receiver.recv().await.unwrap()).unwrap();
        let final_event = recognition_payload(receiver.recv().await.unwrap()).unwrap();

        assert_eq!(partial["type"], "partial");
        assert_eq!(partial["utterance_id"], "utterance-7");
        assert_eq!(partial["text"], "hello");
        assert_eq!(final_event["type"], "subtitle");
        assert_eq!(final_event["utterance_id"], "utterance-7");
        assert_eq!(final_event["subtitle"]["id"], 7);
    }
}
