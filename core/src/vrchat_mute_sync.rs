use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use rosc::{OscPacket, OscType};
use serde::Serialize;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

const SERVICE_TYPE: &str = "_oscjson._tcp.local.";
const MUTE_PATH: &str = "/avatar/parameters/MuteSelf";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VrchatMuteStatus {
    pub enabled: bool,
    pub connection: String,
    pub muted: Option<bool>,
    pub last_error: Option<String>,
}

impl VrchatMuteStatus {
    fn disabled() -> Self {
        Self {
            enabled: false,
            connection: "disabled".into(),
            muted: None,
            last_error: None,
        }
    }

    fn discovering() -> Self {
        Self {
            enabled: true,
            connection: "discovering".into(),
            muted: None,
            last_error: None,
        }
    }
}

#[derive(Clone)]
pub struct VrchatMuteSync {
    enabled: watch::Sender<bool>,
    state: watch::Sender<VrchatMuteStatus>,
    status: Arc<Mutex<VrchatMuteStatus>>,
}

impl VrchatMuteSync {
    pub fn new(enabled: bool, shutdown: watch::Receiver<bool>) -> Self {
        let initial = if enabled {
            VrchatMuteStatus::discovering()
        } else {
            VrchatMuteStatus::disabled()
        };
        let (enabled_tx, enabled_rx) = watch::channel(enabled);
        let (state, _) = watch::channel(initial.clone());
        let status = Arc::new(Mutex::new(initial));
        tokio::spawn(run(
            enabled_rx,
            shutdown,
            state.clone(),
            Arc::clone(&status),
        ));
        Self {
            enabled: enabled_tx,
            state,
            status,
        }
    }

    pub fn update_enabled(&self, enabled: bool) {
        let _ = self.enabled.send(enabled);
    }

    pub fn subscribe(&self) -> watch::Receiver<VrchatMuteStatus> {
        self.state.subscribe()
    }

    pub fn status(&self) -> VrchatMuteStatus {
        self.status.lock().expect("VRChat mute status lock").clone()
    }
}

async fn run(
    mut enabled: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
    state: watch::Sender<VrchatMuteStatus>,
    status: Arc<Mutex<VrchatMuteStatus>>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        if !*enabled.borrow() {
            set_status(&state, &status, VrchatMuteStatus::disabled());
            tokio::select! {
                _ = shutdown.changed() => continue,
                _ = enabled.changed() => continue,
            }
        }

        set_status(&state, &status, VrchatMuteStatus::discovering());
        let result = discover_and_follow(&mut enabled, &mut shutdown, &state, &status).await;
        if let Err(error) = result {
            set_status(
                &state,
                &status,
                VrchatMuteStatus {
                    enabled: true,
                    connection: "unavailable".into(),
                    muted: None,
                    last_error: Some(error),
                },
            );
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(2)) => {},
                _ = shutdown.changed() => {},
                _ = enabled.changed() => {},
            }
        }
    }
}

async fn discover_and_follow(
    enabled: &mut watch::Receiver<bool>,
    shutdown: &mut watch::Receiver<bool>,
    state: &watch::Sender<VrchatMuteStatus>,
    status: &Arc<Mutex<VrchatMuteStatus>>,
) -> Result<(), String> {
    let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
    let services = daemon
        .browse(SERVICE_TYPE)
        .map_err(|error| error.to_string())?;
    let service = loop {
        tokio::select! {
            event = services.recv_async() => match event.map_err(|error| error.to_string())? {
                ServiceEvent::ServiceResolved(info)
                    if info.get_fullname().to_ascii_lowercase().contains("vrchat-client") => break info,
                _ => continue,
            },
            _ = enabled.changed() => {
                let _ = daemon.shutdown();
                return Ok(());
            },
            _ = shutdown.changed() => {
                let _ = daemon.shutdown();
                return Ok(());
            },
        }
    };
    let port = service.get_port();
    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?;
    let muted = query_mute(&client, port).await?;
    set_status(state, status, connected(muted));

    let websocket = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}")).await;
    let Ok((mut websocket, _)) = websocket else {
        return poll_mute(client, port, enabled, shutdown, state, status).await;
    };
    websocket
        .send(Message::Text(
            serde_json::json!({ "COMMAND": "LISTEN", "DATA": MUTE_PATH })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|error| error.to_string())?;

    let mut refresh = tokio::time::interval(Duration::from_secs(5));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            message = websocket.next() => match message {
                Some(Ok(Message::Binary(bytes))) => {
                    if let Some(muted) = decode_mute(&bytes) {
                        set_status(state, status, connected(muted));
                    }
                }
                Some(Ok(Message::Ping(bytes))) => {
                    websocket.send(Message::Pong(bytes)).await.map_err(|error| error.to_string())?;
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Err("VRChat OSCQuery connection closed".into()),
                _ => {}
            },
            _ = refresh.tick() => {
                let muted = query_mute(&client, port).await?;
                set_status(state, status, connected(muted));
            },
            _ = enabled.changed() => return Ok(()),
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

async fn poll_mute(
    client: reqwest::Client,
    port: u16,
    enabled: &mut watch::Receiver<bool>,
    shutdown: &mut watch::Receiver<bool>,
    state: &watch::Sender<VrchatMuteStatus>,
    status: &Arc<Mutex<VrchatMuteStatus>>,
) -> Result<(), String> {
    let mut tick = tokio::time::interval(Duration::from_millis(750));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let muted = query_mute(&client, port).await?;
                set_status(state, status, connected(muted));
            },
            _ = enabled.changed() => return Ok(()),
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

async fn query_mute(client: &reqwest::Client, port: u16) -> Result<bool, String> {
    let value: serde_json::Value = client
        .get(format!("http://127.0.0.1:{port}{MUTE_PATH}?VALUE"))
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    mute_from_json(&value).ok_or_else(|| "VRChat OSCQuery did not return MuteSelf".into())
}

fn mute_from_json(value: &serde_json::Value) -> Option<bool> {
    value
        .get("VALUE")
        .and_then(|value| value.as_array())
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_bool)
        .or_else(|| value.get("VALUE").and_then(serde_json::Value::as_bool))
}

fn decode_mute(bytes: &[u8]) -> Option<bool> {
    let (_, packet) = rosc::decoder::decode_udp(bytes).ok()?;
    let OscPacket::Message(message) = packet else {
        return None;
    };
    if message.addr != MUTE_PATH {
        return None;
    }
    match message.args.first()? {
        OscType::Bool(value) => Some(*value),
        OscType::Int(value) => Some(*value != 0),
        OscType::Float(value) => Some(*value != 0.0),
        _ => None,
    }
}

fn connected(muted: bool) -> VrchatMuteStatus {
    VrchatMuteStatus {
        enabled: true,
        connection: "connected".into(),
        muted: Some(muted),
        last_error: None,
    }
}

fn set_status(
    state: &watch::Sender<VrchatMuteStatus>,
    status: &Arc<Mutex<VrchatMuteStatus>>,
    next: VrchatMuteStatus,
) {
    let mut current = status.lock().expect("VRChat mute status lock");
    if *current == next {
        return;
    }
    *current = next.clone();
    drop(current);
    let _ = state.send(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_oscquery_value_array() {
        assert_eq!(
            mute_from_json(&serde_json::json!({ "VALUE": [true] })),
            Some(true)
        );
    }

    #[test]
    fn decodes_mute_osc_message() {
        let packet = rosc::encoder::encode(&OscPacket::Message(rosc::OscMessage {
            addr: MUTE_PATH.into(),
            args: vec![OscType::Bool(false)],
        }))
        .unwrap();
        assert_eq!(decode_mute(&packet), Some(false));
    }
}
