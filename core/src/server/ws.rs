use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::broadcast;

use super::{api_error, token_eq, AppState, ALLOWED_ORIGINS};

pub(super) async fn ws_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let supplied = params.get("token").map(String::as_str).unwrap_or("");
    if !token_eq(supplied, &state.session_token) {
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

pub(super) async fn handle_socket(state: Arc<AppState>, socket: WebSocket) {
    let mut receiver = state.subtitle_output.subscribe_subtitles();
    let mut live_receiver = state.live_tx.subscribe();
    let mut translation_receiver = state.subtitle_output.subscribe_translations();
    let mut shutdown = state.shutdown.clone();
    let (mut sender, mut incoming) = socket.split();
    if sender
        .send(Message::Text(r#"{"type":"connected"}"#.into()))
        .await
        .is_err()
    {
        return;
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
                        let payload = json!({ "type": "subtitle", "subtitle": subtitle }).to_string();
                        if sender.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            event = live_receiver.recv() => {
                match event {
                    Ok(event) => {
                        let payload = serde_json::to_string(&event).expect("live event serialization");
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
        }
    }
    let _ = sender.send(Message::Close(None)).await;
}
