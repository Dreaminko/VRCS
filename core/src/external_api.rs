use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::config::ExternalApiConfig;
use crate::domain_events::{DomainEvent, DomainEventHub, API_VERSION, EVENT_TYPES};

const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_PROTOCOL: &str = "vrcs.events.v1";
const TOKEN_PROTOCOL_PREFIX: &str = "vrcs.token.";

#[derive(Clone)]
struct ExternalApiState {
    events: DomainEventHub,
    token: Option<String>,
    shutdown: watch::Receiver<bool>,
}

pub struct ExternalApiServer {
    pub address: SocketAddr,
    pub task: JoinHandle<Result<(), String>>,
    config: ExternalApiConfig,
    token: Option<String>,
    stop: watch::Sender<bool>,
}

impl ExternalApiServer {
    pub(crate) async fn stop(self) {
        let _ = self.stop.send(true);
        match self.task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "External API listener stopped with an error"),
            Err(error) => tracing::warn!(%error, "External API listener task exited unexpectedly"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalApiRuntimeStatus {
    pub state: &'static str,
    pub address: Option<String>,
    pub error: Option<String>,
}

impl ExternalApiRuntimeStatus {
    pub fn disabled() -> Self {
        Self {
            state: "disabled",
            address: None,
            error: None,
        }
    }

    pub fn running(address: SocketAddr) -> Self {
        Self {
            state: "running",
            address: Some(address.to_string()),
            error: None,
        }
    }

    pub fn failed(error: String) -> Self {
        Self {
            state: "failed",
            address: None,
            error: Some(error),
        }
    }
}

pub async fn start(
    config: &ExternalApiConfig,
    events: DomainEventHub,
    token: Option<String>,
    shutdown: watch::Receiver<bool>,
) -> Result<Option<ExternalApiServer>, String> {
    if !config.enabled {
        return Ok(None);
    }
    let host: IpAddr = config
        .host
        .parse()
        .map_err(|_| format!("Invalid External API listen address: {}", config.host))?;
    if (!host.is_loopback() || config.require_token) && token.is_none() {
        return Err("External API token is required for the configured listener".into());
    }
    let requested = SocketAddr::new(host, config.port);
    let listener = tokio::net::TcpListener::bind(requested)
        .await
        .map_err(|error| format!("Failed to listen for External API on {requested}: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("Failed to read the External API listen address: {error}"))?;
    let active_token = config.require_token.then_some(token.clone()).flatten();
    let (stop, stop_rx) = watch::channel(false);
    let state = ExternalApiState {
        events,
        token: active_token,
        shutdown: stop_rx.clone(),
    };
    let router = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/events", get(events_handler))
        .with_state(state);
    let mut core_shutdown = shutdown;
    let mut server_stop = stop_rx;
    let connection_stop = stop.clone();
    let task = tokio::spawn(async move {
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                if !*core_shutdown.borrow() && !*server_stop.borrow() {
                    tokio::select! {
                        _ = core_shutdown.changed() => {}
                        _ = server_stop.changed() => {}
                    }
                }
                let _ = connection_stop.send(true);
            })
            .await
            .map_err(|error| format!("External API server failed: {error}"));
        result
    });
    tracing::info!(%address, api_version = API_VERSION, "External API listening");
    Ok(Some(ExternalApiServer {
        address,
        task,
        config: config.clone(),
        token,
        stop,
    }))
}

pub async fn reconfigure(
    server: &mut Option<ExternalApiServer>,
    config: &ExternalApiConfig,
    events: DomainEventHub,
    token: Option<String>,
    shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    if !config.enabled {
        if let Some(previous) = server.take() {
            previous.stop().await;
        }
        return Ok(());
    }

    let stop_first = server
        .as_ref()
        .is_some_and(|current| current.config.port == config.port);
    if !stop_first {
        let candidate = start(config, events, token, shutdown)
            .await?
            .expect("enabled External API returns a server");
        if let Some(previous) = server.replace(candidate) {
            previous.stop().await;
        }
        return Ok(());
    }

    let previous = server.take().expect("stop-first requires a server");
    let previous_config = previous.config.clone();
    let previous_token = previous.token.clone();
    previous.stop().await;
    match start(config, events.clone(), token, shutdown.clone()).await {
        Ok(Some(candidate)) => {
            *server = Some(candidate);
            Ok(())
        }
        Ok(None) => unreachable!("enabled External API returns a server"),
        Err(error) => match start(&previous_config, events, previous_token, shutdown).await {
            Ok(Some(restored)) => {
                *server = Some(restored);
                Err(error)
            }
            Ok(None) => unreachable!("previous External API configuration was enabled"),
            Err(recovery) => Err(format!(
                "{error}; previous listener could not be restored: {recovery}"
            )),
        },
    }
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "api_version": API_VERSION }))
}

async fn capabilities(State(state): State<ExternalApiState>) -> Json<Value> {
    Json(json!({
        "api_version": API_VERSION,
        "events": EVENT_TYPES,
        "authentication_required": state.token.is_some(),
        "subscription_timeout_ms": SUBSCRIBE_TIMEOUT.as_millis(),
    }))
}

async fn events_handler(
    State(state): State<ExternalApiState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if headers.contains_key(header::ORIGIN) && state.token.is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Browser WebSocket connections require token authentication" })),
        )
            .into_response();
    }
    if state
        .token
        .as_deref()
        .is_some_and(|expected| !request_has_token(&headers, expected))
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized" })),
        )
            .into_response();
    }
    ws.protocols([EVENT_PROTOCOL])
        .on_upgrade(move |socket| handle_socket(state, socket))
}

fn request_has_token(headers: &HeaderMap, expected: &str) -> bool {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if bearer.is_some_and(|value| token_eq(value, expected)) {
        return true;
    }
    headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter_map(|protocol| protocol.strip_prefix(TOKEN_PROTOCOL_PREFIX))
        .any(|value| token_eq(value, expected))
}

fn token_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

#[derive(Deserialize)]
struct ClientMessage {
    #[serde(rename = "type")]
    message_type: String,
    events: Option<Vec<String>>,
}

async fn handle_socket(state: ExternalApiState, socket: WebSocket) {
    let connection_id = uuid::Uuid::new_v4().to_string();
    let (mut sender, mut incoming) = socket.split();
    if send_event(
        &mut sender,
        DomainEvent::control(
            "system.connected",
            &connection_id,
            json!({ "connection_id": connection_id }),
        ),
    )
    .await
    .is_err()
    {
        return;
    }

    let mut subscribed = None;
    let deadline = tokio::time::sleep(SUBSCRIBE_TIMEOUT);
    tokio::pin!(deadline);
    while subscribed.is_none() {
        tokio::select! {
            _ = &mut deadline => {
                let _ = send_control_error(&mut sender, &connection_id, "subscription_timeout", "Subscribe within 5 seconds").await;
                let _ = sender.send(Message::Close(None)).await;
                return;
            }
            message = incoming.next() => {
                let Some(Ok(Message::Text(text))) = message else {
                    let _ = sender.send(Message::Close(None)).await;
                    return;
                };
                match parse_subscription(&text) {
                    Ok(events) => {
                        let payload = json!({ "events": ordered_events(&events) });
                        if send_event(&mut sender, DomainEvent::control("system.subscribed", &connection_id, payload)).await.is_err() {
                            return;
                        }
                        subscribed = Some(events);
                    }
                    Err((code, detail)) => {
                        if send_control_error(&mut sender, &connection_id, code, &detail).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
    }

    let mut subscriptions = subscribed.expect("subscription was set");
    let mut events = state.events.subscribe();
    let mut shutdown = state.shutdown;
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                break;
            }
            message = incoming.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => match parse_subscription(&text) {
                        Ok(updated) => {
                            subscriptions = updated;
                            let payload = json!({ "events": ordered_events(&subscriptions) });
                            if send_event(&mut sender, DomainEvent::control("system.subscribed", &connection_id, payload)).await.is_err() {
                                break;
                            }
                        }
                        Err((code, detail)) => {
                            if send_control_error(&mut sender, &connection_id, code, &detail).await.is_err() {
                                break;
                            }
                        }
                    },
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            event = events.recv() => match event {
                Ok(event) if subscriptions.contains(event.event_type.as_str()) => {
                    if send_event(&mut sender, event).await.is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    let _ = send_event(&mut sender, DomainEvent::control(
                        "system.lagged",
                        &connection_id,
                        json!({ "dropped": dropped }),
                    )).await;
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
    let _ = sender.send(Message::Close(None)).await;
}

fn parse_subscription(text: &str) -> Result<HashSet<String>, (&'static str, String)> {
    let message: ClientMessage = serde_json::from_str(text).map_err(|_| {
        (
            "invalid_message",
            "Expected a JSON subscribe message".into(),
        )
    })?;
    if message.message_type != "subscribe" {
        return Err((
            "unsupported_command",
            "Only the subscribe command is supported".into(),
        ));
    }
    let patterns = message.events.unwrap_or_default();
    let mut expanded = HashSet::new();
    for pattern in patterns {
        let matched: Vec<&str> = if pattern == "*" {
            EVENT_TYPES.to_vec()
        } else if let Some(prefix) = pattern.strip_suffix(".*") {
            EVENT_TYPES
                .iter()
                .copied()
                .filter(|event| event.starts_with(&format!("{prefix}.")))
                .collect()
        } else if EVENT_TYPES.contains(&pattern.as_str()) {
            vec![EVENT_TYPES
                .iter()
                .copied()
                .find(|event| *event == pattern)
                .expect("event type exists")]
        } else {
            Vec::new()
        };
        if matched.is_empty() {
            return Err((
                "unknown_event_pattern",
                format!("Unknown event pattern: {pattern}"),
            ));
        }
        expanded.extend(matched.into_iter().map(str::to_owned));
    }
    Ok(expanded)
}

fn ordered_events(events: &HashSet<String>) -> Vec<&'static str> {
    EVENT_TYPES
        .iter()
        .copied()
        .filter(|event| events.contains(*event))
        .collect()
}

async fn send_control_error<S>(
    sender: &mut S,
    connection_id: &str,
    code: &str,
    detail: &str,
) -> Result<(), ()>
where
    S: futures_util::Sink<Message> + Unpin,
{
    send_event(
        sender,
        DomainEvent::control(
            "system.error",
            connection_id,
            json!({ "code": code, "detail": detail }),
        ),
    )
    .await
}

async fn send_event<S>(sender: &mut S, event: DomainEvent) -> Result<(), ()>
where
    S: futures_util::Sink<Message> + Unpin,
{
    let payload = serde_json::to_string(&event).map_err(|_| ())?;
    sender
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    #[test]
    fn subscriptions_expand_supported_patterns() {
        let events =
            parse_subscription(r#"{"type":"subscribe","events":["asr.*","chatbox.sent"]}"#)
                .unwrap();
        assert!(events.contains("asr.partial"));
        assert!(events.contains("asr.final"));
        assert!(events.contains("asr.cancelled"));
        assert!(events.contains("asr.reset"));
        assert!(events.contains("asr.failed"));
        assert!(events.contains("chatbox.sent"));
        assert!(!events.contains("translation.completed"));
    }

    #[test]
    fn subscriptions_reject_unknown_patterns_and_commands() {
        assert!(parse_subscription(r#"{"type":"send","events":["*"]}"#).is_err());
        assert!(parse_subscription(r#"{"type":"subscribe","events":["audio.*"]}"#).is_err());
    }

    #[test]
    fn authentication_accepts_bearer_or_restricted_subprotocol() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        assert!(request_has_token(&headers, "secret"));
        headers.remove(header::AUTHORIZATION);
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            "vrcs.events.v1, vrcs.token.secret".parse().unwrap(),
        );
        assert!(request_has_token(&headers, "secret"));
        assert!(!request_has_token(&headers, "other"));
    }

    #[tokio::test]
    async fn authenticated_listeners_refuse_to_start_without_a_token() {
        let (_, shutdown) = watch::channel(false);
        let result = start(
            &ExternalApiConfig {
                enabled: true,
                require_token: true,
                port: 0,
                ..ExternalApiConfig::default()
            },
            DomainEventHub::new(),
            None,
            shutdown,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn websocket_filters_events_and_preserves_message_correlation() {
        let hub = DomainEventHub::new();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server = start(
            &ExternalApiConfig {
                enabled: true,
                port: 0,
                ..ExternalApiConfig::default()
            },
            hub.clone(),
            None,
            shutdown_rx,
        )
        .await
        .unwrap()
        .unwrap();
        let url = format!("ws://{}/v1/events", server.address);
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();

        let connected: Value =
            serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(connected["type"], "system.connected");
        socket
            .send(ClientMessage::Text(
                r#"{"type":"subscribe","events":["asr.*"]}"#.into(),
            ))
            .await
            .unwrap();
        let subscribed: Value =
            serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(
            subscribed["payload"]["events"],
            json!([
                "asr.partial",
                "asr.final",
                "asr.cancelled",
                "asr.reset",
                "asr.failed"
            ])
        );

        hub.translation_started("utterance-1", "speaker", 1);
        hub.asr_partial("utterance-1", "speaker", "hello", Some("en"));
        let event: Value = serde_json::from_str(
            tokio::time::timeout(Duration::from_secs(1), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .to_text()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(event["type"], "asr.partial");
        assert_eq!(event["message_id"], "utterance-1");
        assert_eq!(event["payload"]["text"], "hello");

        let _ = shutdown_tx.send(true);
        server.task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn token_auth_does_not_accept_query_tokens() {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server = start(
            &ExternalApiConfig {
                enabled: true,
                require_token: true,
                port: 0,
                ..ExternalApiConfig::default()
            },
            DomainEventHub::new(),
            Some("secret".into()),
            shutdown_rx,
        )
        .await
        .unwrap()
        .unwrap();
        let url = format!("ws://{}/v1/events", server.address);
        assert!(
            tokio_tungstenite::connect_async(format!("{url}?token=secret"))
                .await
                .is_err()
        );

        let mut wrong_request = url.clone().into_client_request().unwrap();
        wrong_request.headers_mut().insert(
            header::AUTHORIZATION,
            "Bearer internal-token".parse().unwrap(),
        );
        assert!(tokio_tungstenite::connect_async(wrong_request)
            .await
            .is_err());

        let mut request = url.into_client_request().unwrap();
        request
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let connected: Value =
            serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(connected["type"], "system.connected");

        let _ = shutdown_tx.send(true);
        server.task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn token_changes_reload_the_existing_listener() {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let config = ExternalApiConfig {
            enabled: true,
            require_token: true,
            port,
            ..ExternalApiConfig::default()
        };
        let hub = DomainEventHub::new();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let mut server = start(
            &config,
            hub.clone(),
            Some("old-secret".into()),
            shutdown_rx.clone(),
        )
        .await
        .unwrap();

        reconfigure(
            &mut server,
            &config,
            hub,
            Some("new-secret".into()),
            shutdown_rx,
        )
        .await
        .unwrap();
        let url = format!(
            "ws://{}/v1/events",
            server.as_ref().expect("server running").address
        );
        let mut old_request = url.clone().into_client_request().unwrap();
        old_request
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer old-secret".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(old_request).await.is_err());

        let mut new_request = url.into_client_request().unwrap();
        new_request
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer new-secret".parse().unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(new_request).await.unwrap();
        let connected: Value =
            serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(connected["type"], "system.connected");

        let _ = shutdown_tx.send(true);
        server.take().expect("server running").stop().await;
    }

    #[tokio::test]
    async fn browser_origins_require_token_authentication() {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server = start(
            &ExternalApiConfig {
                enabled: true,
                port: 0,
                ..ExternalApiConfig::default()
            },
            DomainEventHub::new(),
            None,
            shutdown_rx,
        )
        .await
        .unwrap()
        .unwrap();
        let url = format!("ws://{}/v1/events", server.address);
        let mut request = url.into_client_request().unwrap();
        request
            .headers_mut()
            .insert(header::ORIGIN, "https://example.com".parse().unwrap());

        assert!(tokio_tungstenite::connect_async(request).await.is_err());

        let _ = shutdown_tx.send(true);
        server.task.await.unwrap().unwrap();

        let (authenticated_shutdown_tx, authenticated_shutdown_rx) = watch::channel(false);
        let authenticated_server = start(
            &ExternalApiConfig {
                enabled: true,
                require_token: true,
                port: 0,
                ..ExternalApiConfig::default()
            },
            DomainEventHub::new(),
            Some("secret".into()),
            authenticated_shutdown_rx,
        )
        .await
        .unwrap()
        .unwrap();
        let mut authenticated_request = format!("ws://{}/v1/events", authenticated_server.address)
            .into_client_request()
            .unwrap();
        authenticated_request
            .headers_mut()
            .insert(header::ORIGIN, "https://example.com".parse().unwrap());
        authenticated_request.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            "vrcs.events.v1, vrcs.token.secret".parse().unwrap(),
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(authenticated_request)
            .await
            .unwrap();
        let connected: Value =
            serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(connected["type"], "system.connected");

        let _ = authenticated_shutdown_tx.send(true);
        authenticated_server.task.await.unwrap().unwrap();
    }
}
