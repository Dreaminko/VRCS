use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{header::AUTHORIZATION, HeaderValue};
use tokio_tungstenite::tungstenite::Message;

use crate::config::{AsrConfig, VrcxConfig};
use crate::translation::TranslationContextEntry;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ASR_CONTEXT_CHARS: usize = 400;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VrcxRuntimeStatus {
    pub state: &'static str,
    pub app_version: Option<String>,
    pub protocol: Option<u32>,
    pub world_name: Option<String>,
    pub member_count: usize,
    pub last_updated_at: Option<String>,
    pub error: Option<String>,
}

impl VrcxRuntimeStatus {
    fn disabled() -> Self {
        Self::with_state("disabled")
    }

    fn with_state(state: &'static str) -> Self {
        Self {
            state,
            app_version: None,
            protocol: None,
            world_name: None,
            member_count: 0,
            last_updated_at: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrcxRoomContext {
    world_name: String,
    members: Vec<VrcxMemberContext>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VrcxMemberContext {
    user_id: String,
    display_name: String,
    is_self: bool,
    languages: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireRoom {
    #[serde(default)]
    world_name: String,
    #[serde(default)]
    members: Vec<WireMember>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireMember {
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    is_self: bool,
    #[serde(default)]
    languages: Vec<String>,
}

struct ConnectorTask {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

struct Shared {
    status: RwLock<VrcxRuntimeStatus>,
    room: RwLock<Option<VrcxRoomContext>>,
}

#[derive(Clone)]
pub struct VrcxIntegration {
    shared: Arc<Shared>,
    task: Arc<Mutex<Option<ConnectorTask>>>,
    shutdown: watch::Receiver<bool>,
}

impl VrcxIntegration {
    pub fn new(shutdown: watch::Receiver<bool>) -> Self {
        Self {
            shared: Arc::new(Shared {
                status: RwLock::new(VrcxRuntimeStatus::disabled()),
                room: RwLock::new(None),
            }),
            task: Arc::new(Mutex::new(None)),
            shutdown,
        }
    }

    pub async fn reconfigure(&self, config: VrcxConfig, token: Option<String>) {
        let mut task = self.task.lock().await;
        if let Some(previous) = task.take() {
            let _ = previous.stop.send(true);
            previous.task.abort();
            let _ = previous.task.await;
        }
        self.clear_room();
        if !config.enabled {
            self.set_status(VrcxRuntimeStatus::disabled());
            return;
        }
        let Some(token) = token.filter(|value| !value.trim().is_empty()) else {
            self.set_status(VrcxRuntimeStatus::with_state("missing_token"));
            return;
        };
        self.set_status(VrcxRuntimeStatus::with_state("connecting"));
        let (stop, stop_rx) = watch::channel(false);
        let shared = Arc::clone(&self.shared);
        let shutdown = self.shutdown.clone();
        let connector_config = config.clone();
        let connector_token = token.clone();
        let handle = tokio::spawn(async move {
            run_connector(shared, connector_config, connector_token, shutdown, stop_rx).await;
        });
        *task = Some(ConnectorTask { stop, task: handle });
    }

    pub fn status(&self) -> VrcxRuntimeStatus {
        self.shared.status.read().expect("VRCX status lock").clone()
    }

    pub fn translation_context_entry(&self) -> Option<TranslationContextEntry> {
        let room = self.shared.room.read().expect("VRCX room lock").clone()?;
        let text = format_room_for_llm(&room);
        (!text.is_empty()).then(|| TranslationContextEntry {
            source: "vrcx_room".into(),
            text,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    pub fn apply_asr_context(&self, config: &mut AsrConfig) {
        let Some(room) = self.shared.room.read().expect("VRCX room lock").clone() else {
            return;
        };
        let generated = format_room_for_asr(&room);
        if generated.is_empty() {
            return;
        }
        match config.backend.as_str() {
            "qwen_realtime" => {
                config.qwen.context = append_context(&config.qwen.context, &generated);
            }
            "fun_asr_realtime" => {
                config.fun_asr.context = append_context(&config.fun_asr.context, &generated);
            }
            _ => {}
        }
    }

    pub async fn test_connection(
        config: &VrcxConfig,
        token: &str,
    ) -> Result<VrcxRuntimeStatus, String> {
        let mut socket = connect(config.port, token).await?;
        let hello = receive_json(&mut socket, CONNECT_TIMEOUT).await?;
        let (app_version, protocol, heartbeat) = parse_hello(&hello)?;
        socket
            .send(Message::Text(
                json!({ "type": "resync" }).to_string().into(),
            ))
            .await
            .map_err(|error| format!("Failed to request VRCX-0 room snapshot: {error}"))?;
        let deadline = Duration::from_secs(5).min(heartbeat);
        for _ in 0..5 {
            let value = receive_json(&mut socket, deadline).await?;
            if message_type(&value) == Some("room.snapshot") {
                let room = parse_room_snapshot(&value)?;
                return Ok(status_from_room(app_version, protocol, &room));
            }
        }
        Err("VRCX-0 did not provide a room snapshot".into())
    }

    fn clear_room(&self) {
        *self.shared.room.write().expect("VRCX room lock") = None;
    }

    fn set_status(&self, status: VrcxRuntimeStatus) {
        *self.shared.status.write().expect("VRCX status lock") = status;
    }
}

async fn run_connector(
    shared: Arc<Shared>,
    config: VrcxConfig,
    token: String,
    mut shutdown: watch::Receiver<bool>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = Duration::from_millis(500);
    loop {
        set_shared_status(&shared, VrcxRuntimeStatus::with_state("connecting"));
        let result = run_connection(&shared, &config, &token, &mut shutdown, &mut stop).await;
        if *shutdown.borrow() || *stop.borrow() {
            return;
        }
        clear_shared_room(&shared);
        let mut status = VrcxRuntimeStatus::with_state("error");
        status.error = Some(
            result
                .err()
                .unwrap_or_else(|| "VRCX-0 connection closed".into()),
        );
        set_shared_status(&shared, status);
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = shutdown.changed() => return,
            _ = stop.changed() => return,
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn run_connection(
    shared: &Arc<Shared>,
    config: &VrcxConfig,
    token: &str,
    shutdown: &mut watch::Receiver<bool>,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let mut socket = connect(config.port, token).await?;
    let hello = receive_json(&mut socket, CONNECT_TIMEOUT).await?;
    let (app_version, protocol, heartbeat) = parse_hello(&hello)?;
    socket
        .send(Message::Text(
            json!({ "type": "resync" }).to_string().into(),
        ))
        .await
        .map_err(|error| format!("Failed to request VRCX-0 room snapshot: {error}"))?;
    let mut last_seq = 0_u64;
    let idle_timeout = heartbeat.saturating_mul(2).max(Duration::from_secs(10));
    loop {
        let message = tokio::select! {
            _ = shutdown.changed() => return Ok(()),
            _ = stop.changed() => return Ok(()),
            result = tokio::time::timeout(idle_timeout, socket.next()) => {
                result.map_err(|_| "VRCX-0 heartbeat timed out".to_string())?
                    .ok_or_else(|| "VRCX-0 connection closed".to_string())?
                    .map_err(|error| format!("VRCX-0 WebSocket error: {error}"))?
            }
        };
        match message {
            Message::Text(text) => {
                let value: Value = serde_json::from_str(text.as_ref())
                    .map_err(|error| format!("Invalid VRCX-0 message: {error}"))?;
                if let Some(seq) = value.get("seq").and_then(Value::as_u64) {
                    if last_seq != 0 && seq != last_seq + 1 {
                        socket
                            .send(Message::Text(
                                json!({ "type": "resync" }).to_string().into(),
                            ))
                            .await
                            .map_err(|error| format!("Failed to resync VRCX-0: {error}"))?;
                    }
                    last_seq = seq;
                }
                match message_type(&value) {
                    Some("room.snapshot") => {
                        let room = parse_room_snapshot(&value)?;
                        *shared.room.write().expect("VRCX room lock") = room.clone();
                        set_shared_status(
                            shared,
                            status_from_room(app_version.clone(), protocol, &room),
                        );
                    }
                    Some("room.joined") => update_joined(shared, &value, &app_version, protocol),
                    Some("room.left") => update_left(shared, &value, &app_version, protocol),
                    Some("bye") => return Err("VRCX-0 integration became unavailable".into()),
                    _ => {}
                }
            }
            Message::Ping(payload) => socket
                .send(Message::Pong(payload))
                .await
                .map_err(|error| format!("Failed to answer VRCX-0 ping: {error}"))?,
            Message::Close(_) => return Err("VRCX-0 connection closed".into()),
            _ => {}
        }
    }
}

async fn connect(
    port: u16,
    token: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    String,
> {
    let url = format!("ws://127.0.0.1:{port}/v1/stream");
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("Invalid VRCX-0 WebSocket URL: {error}"))?;
    let authorization = HeaderValue::from_str(&format!("Bearer {}", token.trim()))
        .map_err(|_| "VRCX-0 token contains invalid header characters".to_string())?;
    request.headers_mut().insert(AUTHORIZATION, authorization);
    let (socket, _) =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
            .await
            .map_err(|_| "Timed out connecting to VRCX-0".to_string())?
            .map_err(|error| format!("Failed to connect to VRCX-0: {error}"))?;
    Ok(socket)
}

async fn receive_json<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    timeout: Duration,
) -> Result<Value, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        let message = tokio::time::timeout(timeout, socket.next())
            .await
            .map_err(|_| "Timed out waiting for VRCX-0".to_string())?
            .ok_or_else(|| "VRCX-0 connection closed".to_string())?
            .map_err(|error| format!("VRCX-0 WebSocket error: {error}"))?;
        match message {
            Message::Text(text) => {
                return serde_json::from_str(text.as_ref())
                    .map_err(|error| format!("Invalid VRCX-0 message: {error}"));
            }
            Message::Ping(payload) => socket
                .send(Message::Pong(payload))
                .await
                .map_err(|error| format!("Failed to answer VRCX-0 ping: {error}"))?,
            Message::Close(_) => return Err("VRCX-0 connection closed".into()),
            _ => {}
        }
    }
}

fn parse_hello(value: &Value) -> Result<(String, u32, Duration), String> {
    if message_type(value) != Some("hello") {
        return Err("VRCX-0 did not send hello as the first message".into());
    }
    let protocol = value
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| "VRCX-0 hello is missing protocol".to_string())? as u32;
    if protocol != 1 {
        return Err(format!("Unsupported VRCX-0 protocol version: {protocol}"));
    }
    let has_room_scope = value
        .get("scopes")
        .and_then(Value::as_array)
        .is_some_and(|scopes| scopes.iter().any(|scope| scope.as_str() == Some("room")));
    if !has_room_scope {
        return Err("VRCX-0 integration does not expose the room scope".into());
    }
    let app_version = value
        .get("appVersion")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let heartbeat = Duration::from_secs(
        value
            .get("heartbeatSec")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .max(1),
    );
    Ok((app_version, protocol, heartbeat))
}

fn parse_room_snapshot(value: &Value) -> Result<Option<VrcxRoomContext>, String> {
    let Some(room) = value.get("room") else {
        return Ok(None);
    };
    if room.is_null() {
        return Ok(None);
    }
    let room: WireRoom = serde_json::from_value(room.clone())
        .map_err(|error| format!("Invalid VRCX-0 room snapshot: {error}"))?;
    Ok(Some(room.into()))
}

fn update_joined(shared: &Arc<Shared>, value: &Value, app_version: &str, protocol: u32) {
    let member = value
        .get("member")
        .cloned()
        .and_then(|member| serde_json::from_value::<WireMember>(member).ok())
        .map(VrcxMemberContext::from);
    let Some(member) = member else { return };
    let snapshot = {
        let mut guard = shared.room.write().expect("VRCX room lock");
        let Some(room) = guard.as_mut() else { return };
        room.members
            .retain(|existing| existing.user_id != member.user_id);
        room.members.push(member);
        room.clone()
    };
    set_shared_status(
        shared,
        status_from_room(app_version.to_string(), protocol, &Some(snapshot)),
    );
}

fn update_left(shared: &Arc<Shared>, value: &Value, app_version: &str, protocol: u32) {
    let Some(user_id) = value.get("userId").and_then(Value::as_str) else {
        return;
    };
    let snapshot = {
        let mut guard = shared.room.write().expect("VRCX room lock");
        let Some(room) = guard.as_mut() else { return };
        room.members.retain(|member| member.user_id != user_id);
        room.clone()
    };
    set_shared_status(
        shared,
        status_from_room(app_version.to_string(), protocol, &Some(snapshot)),
    );
}

fn status_from_room(
    app_version: String,
    protocol: u32,
    room: &Option<VrcxRoomContext>,
) -> VrcxRuntimeStatus {
    VrcxRuntimeStatus {
        state: "connected",
        app_version: (!app_version.is_empty()).then_some(app_version),
        protocol: Some(protocol),
        world_name: room
            .as_ref()
            .map(|room| room.world_name.clone())
            .filter(|name| !name.is_empty()),
        member_count: room.as_ref().map_or(0, |room| room.members.len()),
        last_updated_at: Some(chrono::Utc::now().to_rfc3339()),
        error: None,
    }
}

fn format_room_for_llm(room: &VrcxRoomContext) -> String {
    let mut lines = Vec::new();
    if !room.world_name.trim().is_empty() {
        lines.push(format!("World: {}", json_string(room.world_name.trim())));
    }
    let mut seen = HashSet::new();
    for member in &room.members {
        let name = member.display_name.trim();
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        let mut details = Vec::new();
        if member.is_self {
            details.push("self".to_string());
        }
        let languages = member
            .languages
            .iter()
            .map(|language| language.trim())
            .filter(|language| !language.is_empty())
            .collect::<Vec<_>>();
        if !languages.is_empty() {
            details.push(format!("languages: {}", languages.join(", ")));
        }
        lines.push(if details.is_empty() {
            format!("Member: {}", json_string(name))
        } else {
            format!("Member: {} [{}]", json_string(name), details.join("; "))
        });
    }
    lines.join("\n")
}

fn format_room_for_asr(room: &VrcxRoomContext) -> String {
    let mut parts = Vec::new();
    if !room.world_name.trim().is_empty() {
        parts.push(format!("World: {}", room.world_name.trim()));
    }
    let mut seen = HashSet::new();
    let names = room
        .members
        .iter()
        .map(|member| member.display_name.trim())
        .filter(|name| !name.is_empty() && seen.insert(name.to_lowercase()))
        .collect::<Vec<_>>();
    if !names.is_empty() {
        parts.push(format!("Names: {}", names.join(", ")));
    }
    parts.join("; ")
}

fn append_context(manual: &str, generated: &str) -> String {
    let manual = manual.trim();
    if manual.chars().count() >= MAX_ASR_CONTEXT_CHARS {
        return manual.to_string();
    }
    let separator = if manual.is_empty() { "" } else { "\n" };
    let remaining = MAX_ASR_CONTEXT_CHARS - manual.chars().count() - separator.chars().count();
    let generated = generated.chars().take(remaining).collect::<String>();
    format!("{manual}{separator}{generated}")
}

fn message_type(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

fn set_shared_status(shared: &Arc<Shared>, status: VrcxRuntimeStatus) {
    *shared.status.write().expect("VRCX status lock") = status;
}

fn clear_shared_room(shared: &Arc<Shared>) {
    *shared.room.write().expect("VRCX room lock") = None;
}

impl From<WireRoom> for VrcxRoomContext {
    fn from(room: WireRoom) -> Self {
        Self {
            world_name: room.world_name,
            members: room.members.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<WireMember> for VrcxMemberContext {
    fn from(member: WireMember) -> Self {
        Self {
            user_id: member.user_id,
            display_name: member.display_name,
            is_self: member.is_self,
            languages: member.languages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> VrcxRoomContext {
        VrcxRoomContext {
            world_name: "Test \"World\"".into(),
            members: vec![
                VrcxMemberContext {
                    user_id: "usr-1".into(),
                    display_name: "Alice".into(),
                    is_self: true,
                    languages: vec!["en".into(), "ja".into()],
                },
                VrcxMemberContext {
                    user_id: "usr-2".into(),
                    display_name: "Bob".into(),
                    is_self: false,
                    languages: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn llm_context_exposes_only_useful_room_fields() {
        let text = format_room_for_llm(&room());
        assert!(text.contains("Test \\\"World\\\""));
        assert!(text.contains("Alice"));
        assert!(text.contains("languages: en, ja"));
        assert!(!text.contains("usr-1"));
    }

    #[test]
    fn asr_context_preserves_manual_text_and_limit() {
        let generated = format_room_for_asr(&room());
        let merged = append_context("VRChat terms", &generated);
        assert!(merged.starts_with("VRChat terms\n"));
        assert!(merged.contains("Alice"));
        assert!(merged.chars().count() <= MAX_ASR_CONTEXT_CHARS);

        let full = "x".repeat(MAX_ASR_CONTEXT_CHARS);
        assert_eq!(append_context(&full, &generated), full);
    }

    #[test]
    fn applies_room_context_only_to_supported_asr_backends() {
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let integration = VrcxIntegration::new(shutdown_rx);
        *integration.shared.room.write().unwrap() = Some(room());

        let mut qwen = AsrConfig::default();
        qwen.backend = "qwen_realtime".into();
        qwen.qwen.context = "Manual terms".into();
        integration.apply_asr_context(&mut qwen);
        assert!(qwen.qwen.context.starts_with("Manual terms\n"));
        assert!(qwen.qwen.context.contains("Alice"));

        let mut fun_asr = AsrConfig::default();
        fun_asr.backend = "fun_asr_realtime".into();
        integration.apply_asr_context(&mut fun_asr);
        assert!(fun_asr.fun_asr.context.contains("Test \"World\""));
        assert!(fun_asr.fun_asr.context.contains("Bob"));

        let mut openai = AsrConfig::default();
        openai.backend = "openai_realtime".into();
        integration.apply_asr_context(&mut openai);
        assert!(openai.qwen.context.is_empty());
        assert!(openai.fun_asr.context.is_empty());
    }

    #[test]
    fn snapshot_parser_accepts_actual_wire_shape() {
        let value = json!({
            "type": "room.snapshot",
            "room": {
                "worldName": "World",
                "location": "private location",
                "members": [{
                    "userId": "usr-1",
                    "displayName": "Alice",
                    "isSelf": true,
                    "languages": ["en"],
                    "note": "private"
                }]
            }
        });
        let room = parse_room_snapshot(&value).unwrap().unwrap();
        assert_eq!(room.world_name, "World");
        assert_eq!(room.members[0].display_name, "Alice");
    }
}
