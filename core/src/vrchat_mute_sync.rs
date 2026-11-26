use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rosc::{OscPacket, OscType};
use serde::Serialize;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

use crate::osc::oscquery::{OscQueryConsumer, OscQueryDiscovery, connect_ws_and_listen};

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
    discovery: OscQueryDiscovery,
}

impl VrchatMuteSync {
    pub(crate) fn new_with_discovery(
        enabled: bool,
        shutdown: watch::Receiver<bool>,
        discovery: OscQueryDiscovery,
    ) -> Self {
        discovery.set_enabled(OscQueryConsumer::MuteSync, enabled);
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
            discovery.clone(),
        ));
        Self {
            enabled: enabled_tx,
            state,
            status,
            discovery,
        }
    }

    pub fn update_enabled(&self, enabled: bool) {
        self.discovery
            .set_enabled(OscQueryConsumer::MuteSync, enabled);
        let _ = self.enabled.send(enabled);
    }

    pub fn subscribe(&self) -> watch::Receiver<VrchatMuteStatus> {
        self.state.subscribe()
    }

    pub fn status(&self) -> VrchatMuteStatus {
        self.status.lock().expect("VRChat mute status lock").clone()
    }

    #[cfg(test)]
    pub(crate) fn set_muted_for_test(&self, muted: bool) {
        set_status(&self.state, &self.status, connected(muted));
    }
}

async fn run(
    mut enabled: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
    state: watch::Sender<VrchatMuteStatus>,
    status: Arc<Mutex<VrchatMuteStatus>>,
    discovery: OscQueryDiscovery,
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
        let result =
            discover_and_follow(&mut enabled, &mut shutdown, &state, &status, &discovery).await;
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
    discovery: &OscQueryDiscovery,
) -> Result<(), String> {
    let mut endpoint_rx = discovery.subscribe();
    let port = loop {
        if let Some(port) = *endpoint_rx.borrow_and_update() {
            break port;
        }
        tokio::select! {
            changed = endpoint_rx.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
            _ = enabled.changed() => return Ok(()),
            _ = shutdown.changed() => return Ok(()),
        }
    };

    let client = crate::http_client_builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?;
    let muted = match query_mute(&client, port).await {
        Ok(muted) => muted,
        Err(error) => {
            discovery.report_unavailable(port);
            return Err(error);
        }
    };
    set_status(state, status, connected(muted));

    let mut websocket = match connect_ws_and_listen(port, &[MUTE_PATH]).await {
        Ok(websocket) => websocket,
        Err(error) => {
            tracing::debug!(
                %error,
                "VRChat OSCQuery WebSocket unavailable; falling back to HTTP polling"
            );
            let result = poll_mute(client, port, enabled, shutdown, state, status).await;
            if result.is_err() {
                discovery.report_unavailable(port);
            }
            return result;
        }
    };

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
                    if let Err(error) = websocket.send(Message::Pong(bytes)).await {
                        discovery.report_unavailable(port);
                        return Err(error.to_string());
                    }
                }
                Some(Ok(Message::Close(_))) | None => {
                    discovery.report_unavailable(port);
                    return Err("VRChat OSCQuery connection closed".into());
                }
                Some(Err(error)) => {
                    discovery.report_unavailable(port);
                    return Err(format!("VRChat OSCQuery WebSocket error: {error}"));
                }
                _ => {}
            },
            _ = refresh.tick() => {
                let muted = match query_mute(&client, port).await {
                    Ok(muted) => muted,
                    Err(error) => {
                        discovery.report_unavailable(port);
                        return Err(error);
                    }
                };
                set_status(state, status, connected(muted));
            },
            changed = endpoint_rx.changed() => {
                if changed.is_err() || *endpoint_rx.borrow() != Some(port) {
                    return Ok(());
                }
            }
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
