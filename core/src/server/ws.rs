use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::domain_events::DomainEvent;
use crate::models::LiveTranscription;

use super::{api_error, token_eq, RealtimeContext, ALLOWED_ORIGINS};

pub(super) async fn ws_handler(
    State(state): State<RealtimeContext>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let supplied = params.get("token").map(String::as_str).unwrap_or("");
    if !token_eq(supplied, &state.integrations.session_token) {
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
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

pub(super) async fn handle_socket(state: RealtimeContext, socket: WebSocket) {
    let mut receiver = state.content.subtitle_output.subscribe_subtitles();
    let mut recognition_receiver = state.integrations.domain_events.subscribe();
    let mut live_receiver = state.capture.live_tx.subscribe();
    let mut catalog_receiver = state.content.conversation_catalog_tx.subscribe();
    let mut translation_receiver = state.content.subtitle_output.subscribe_translations();
    let mut mute_receiver = state.integrations.vrchat_mute_sync.subscribe();
    let mut shutdown = state.integrations.shutdown.clone();
    let (mut sender, mut incoming) = socket.split();
    if sender
        .send(Message::Text(r#"{"type":"connected"}"#.into()))
        .await
        .is_err()
    {
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
            if sender.send(Message::Text(payload.into())).await.is_err() {
                return;
            }
        }
        Err(error) => tracing::warn!(%error, "initial conversation catalog could not be sent"),
    }
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
                        if sender.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            event = recognition_receiver.recv() => {
                match event {
                    Ok(event) => {
                        let Some(payload) = recognition_payload(event) else {
                            continue;
                        };
                        if sender.send(Message::Text(payload.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        tracing::warn!(dropped, "recognition event stream lagged; resetting client state");
                        let payload = json!({ "type": "recognition_reset" }).to_string();
                        if sender.send(Message::Text(payload.into())).await.is_err() {
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
                        if sender.send(Message::Text(payload.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(LiveTranscription::Partial { .. } | LiveTranscription::Failed { .. }) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
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
                        if sender.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            event = translation_receiver.recv() => {
                match event {
                    Ok(event) => {
                        let payload = serde_json::to_string(&event).expect("translation event serialization");
                        if sender.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
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
                if sender.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = sender.send(Message::Close(None)).await;
}

fn recognition_payload(event: DomainEvent) -> Option<Value> {
    match event.event_type.as_str() {
        "asr.partial" => Some(json!({
            "type": "partial",
            "utterance_id": event.message_id,
            "source": event.source,
            "text": event.payload.get("text")?,
            "language": event.payload.get("language"),
        })),
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
    use crate::models::{now_iso8601, Subtitle};

    fn subtitle() -> Subtitle {
        Subtitle {
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
