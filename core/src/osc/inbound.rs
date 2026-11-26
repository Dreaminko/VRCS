use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType};
use serde::Serialize;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::chatbox::compact_text;
use crate::config::OscConfig;
use crate::models::now_iso8601;

use super::transport::CHATBOX_ADDRESS;

const MAX_BUNDLE_DEPTH: usize = 8;
const CHATBOX_QUEUE_CAPACITY: usize = 16;
const DUPLICATE_WINDOW: Duration = Duration::from_millis(1_000);

pub(crate) type InboundChatboxCallback =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct OscInboundRuntimeStatus {
    pub enabled: bool,
    pub state: &'static str,
    pub listen_address: Option<String>,
    pub received_packets: u64,
    pub intercepted_chatbox_messages: u64,
    pub forwarded_packets: u64,
    pub dropped_messages: u64,
    pub decode_errors: u64,
    pub last_received_at: Option<String>,
    pub last_error: Option<String>,
}

impl OscInboundRuntimeStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            state: "disabled",
            listen_address: None,
            received_packets: 0,
            intercepted_chatbox_messages: 0,
            forwarded_packets: 0,
            dropped_messages: 0,
            decode_errors: 0,
            last_received_at: None,
            last_error: None,
        }
    }

    pub fn listening(address: SocketAddr) -> Self {
        Self {
            enabled: true,
            state: "listening",
            listen_address: Some(address.to_string()),
            received_packets: 0,
            intercepted_chatbox_messages: 0,
            forwarded_packets: 0,
            dropped_messages: 0,
            decode_errors: 0,
            last_received_at: None,
            last_error: None,
        }
    }

    pub fn failed(error: String) -> Self {
        Self {
            enabled: true,
            state: "error",
            listen_address: None,
            received_packets: 0,
            intercepted_chatbox_messages: 0,
            forwarded_packets: 0,
            dropped_messages: 0,
            decode_errors: 0,
            last_received_at: None,
            last_error: Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OscInboundRules {
    pub intercept_chatbox_input: bool,
    pub forward_unmatched: bool,
    pub forward_port: u16,
}

impl OscInboundRules {
    pub(crate) fn from_config(config: &OscConfig) -> Self {
        Self {
            intercept_chatbox_input: config.inbound.intercept_chatbox_input,
            forward_unmatched: config.inbound.forward_unmatched,
            forward_port: config.port,
        }
    }
}

pub(crate) struct OscInboundServer {
    #[cfg(test)]
    pub address: SocketAddr,
    port: u16,
    rules: watch::Sender<OscInboundRules>,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl OscInboundServer {
    pub(crate) async fn start(
        config: &OscConfig,
        on_chatbox: InboundChatboxCallback,
        status: Arc<RwLock<OscInboundRuntimeStatus>>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<Option<Self>, String> {
        if !config.inbound.enabled {
            *status.write().expect("OSC inbound status lock") = OscInboundRuntimeStatus::disabled();
            return Ok(None);
        }
        let (socket, forward_socket, address) = bind_sockets(config).await?;
        Ok(Some(Self::from_sockets(
            config,
            socket,
            forward_socket,
            address,
            on_chatbox,
            status,
            shutdown,
        )))
    }

    fn from_sockets(
        config: &OscConfig,
        socket: UdpSocket,
        forward_socket: UdpSocket,
        address: SocketAddr,
        on_chatbox: InboundChatboxCallback,
        status: Arc<RwLock<OscInboundRuntimeStatus>>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        *status.write().expect("OSC inbound status lock") =
            OscInboundRuntimeStatus::listening(address);

        let (rules_tx, rules_rx) = watch::channel(OscInboundRules::from_config(config));
        let (stop_tx, stop_rx) = watch::channel(false);
        let (chatbox_tx, chatbox_rx) = mpsc::channel(CHATBOX_QUEUE_CAPACITY);

        let listener_task = tokio::spawn(run_listener(
            socket,
            forward_socket,
            address.port(),
            rules_rx,
            chatbox_tx,
            Arc::clone(&status),
            stop_rx.clone(),
            shutdown.clone(),
        ));
        let worker_task = tokio::spawn(run_chatbox_worker(
            chatbox_rx, on_chatbox, status, stop_rx, shutdown,
        ));

        tracing::info!(%address, forward_port = config.port, "OSC inbound listener started");
        Self {
            #[cfg(test)]
            address,
            port: config.inbound.port,
            rules: rules_tx,
            stop: stop_tx,
            tasks: vec![listener_task, worker_task],
        }
    }

    pub(crate) fn update_rules(&self, config: &OscConfig) {
        let next = OscInboundRules::from_config(config);
        self.rules.send_if_modified(|current| {
            if *current == next {
                return false;
            }
            *current = next;
            true
        });
    }

    pub(crate) async fn stop(self) {
        let _ = self.stop.send(true);
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

async fn bind_sockets(config: &OscConfig) -> Result<(UdpSocket, UdpSocket, SocketAddr), String> {
    let listen_port = config.inbound.port;
    if listen_port == 0 {
        return Err("OSC inbound port must be between 1 and 65535".into());
    }
    if listen_port == config.port {
        return Err("OSC inbound port cannot match the VRChat OSC output port".into());
    }
    let requested = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, listen_port));
    let socket = UdpSocket::bind(requested)
        .await
        .map_err(|error| format!("Failed to listen for OSC inbound on {requested}: {error}"))?;
    let address = socket
        .local_addr()
        .map_err(|error| format!("Failed to read OSC inbound listen address: {error}"))?;
    let forward_socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("Failed to create OSC forward socket: {error}"))?;
    Ok((socket, forward_socket, address))
}

pub(crate) async fn reconfigure(
    server: &mut Option<OscInboundServer>,
    config: &OscConfig,
    on_chatbox: InboundChatboxCallback,
    status: Arc<RwLock<OscInboundRuntimeStatus>>,
    shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    if !config.inbound.enabled {
        if let Some(previous) = server.take() {
            previous.stop().await;
        }
        *status.write().expect("OSC inbound status lock") = OscInboundRuntimeStatus::disabled();
        return Ok(());
    }

    if let Some(current) = server.as_ref()
        && current.port == config.inbound.port
    {
        current.update_rules(config);
        return Ok(());
    }

    let (socket, forward_socket, address) = bind_sockets(config).await?;
    if let Some(previous) = server.take() {
        previous.stop().await;
    }
    *server = Some(OscInboundServer::from_sockets(
        config,
        socket,
        forward_socket,
        address,
        on_chatbox,
        status,
        shutdown,
    ));
    Ok(())
}

#[derive(Debug)]
struct RoutedDatagram {
    intercepted_chatbox: Vec<String>,
    passthrough: Option<Vec<u8>>,
}

fn route_datagram(bytes: &[u8], rules: &OscInboundRules) -> Result<RoutedDatagram, String> {
    let (_, packet) = rosc::decoder::decode_udp(bytes).map_err(|error| error.to_string())?;
    let mut intercepted_chatbox = Vec::new();
    let mut passthrough_packets = Vec::new();
    collect_messages(
        packet,
        rules.intercept_chatbox_input,
        0,
        &mut intercepted_chatbox,
        &mut passthrough_packets,
    )?;

    let passthrough = if !rules.forward_unmatched || passthrough_packets.is_empty() {
        None
    } else if intercepted_chatbox.is_empty() {
        // Nothing was intercepted: forward raw bytes verbatim with zero re-encoding.
        Some(bytes.to_vec())
    } else if passthrough_packets.len() == 1 {
        Some(
            rosc::encoder::encode(&passthrough_packets.remove(0))
                .map_err(|error| error.to_string())?,
        )
    } else {
        let bundle = OscPacket::Bundle(OscBundle {
            timetag: OscTime {
                seconds: 0,
                fractional: 1,
            },
            content: passthrough_packets,
        });
        Some(rosc::encoder::encode(&bundle).map_err(|error| error.to_string())?)
    };

    Ok(RoutedDatagram {
        intercepted_chatbox,
        passthrough,
    })
}

fn collect_messages(
    packet: OscPacket,
    intercept_chatbox_input: bool,
    depth: usize,
    intercepted: &mut Vec<String>,
    passthrough: &mut Vec<OscPacket>,
) -> Result<(), String> {
    if depth > MAX_BUNDLE_DEPTH {
        return Err("OSC bundle nesting depth exceeded".into());
    }
    match packet {
        OscPacket::Message(message) => {
            if intercept_chatbox_input && let Some(text) = parse_immediate_chatbox_input(&message) {
                intercepted.push(text);
            } else {
                passthrough.push(OscPacket::Message(message));
            }
        }
        OscPacket::Bundle(bundle) => {
            for item in bundle.content {
                collect_messages(
                    item,
                    intercept_chatbox_input,
                    depth + 1,
                    intercepted,
                    passthrough,
                )?;
            }
        }
    }
    Ok(())
}

/// Parses VRChat `/chatbox/input` `(s: text, b: immediate, [b: sfx])`.
/// Only non-empty immediate submissions are intercepted; keyboard pre-fill (`immediate = false`)
/// and clear commands (`""`) pass through untouched.
fn parse_immediate_chatbox_input(message: &OscMessage) -> Option<String> {
    if message.addr != CHATBOX_ADDRESS {
        return None;
    }
    let OscType::String(text) = message.args.first()? else {
        return None;
    };
    let immediate = match message.args.get(1) {
        Some(OscType::Bool(value)) => *value,
        Some(OscType::Int(value)) => *value != 0,
        _ => false,
    };
    if !immediate || text.trim().is_empty() {
        return None;
    }
    Some(text.clone())
}

#[allow(clippy::too_many_arguments)]
async fn run_listener(
    socket: UdpSocket,
    forward_socket: UdpSocket,
    listen_port: u16,
    rules_rx: watch::Receiver<OscInboundRules>,
    chatbox_tx: mpsc::Sender<String>,
    status: Arc<RwLock<OscInboundRuntimeStatus>>,
    mut stop: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut buf = vec![0u8; 65_535];
    let mut last_intercepted: Option<(String, Instant)> = None;

    loop {
        if *stop.borrow() || *shutdown.borrow() {
            break;
        }
        let len = tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = shutdown.changed() => break,
            result = socket.recv_from(&mut buf) => match result {
                Ok((len, _)) => len,
                Err(error) => {
                    tracing::warn!(%error, "OSC inbound UDP receive error");
                    status.write().expect("OSC inbound status lock").last_error =
                        Some(error.to_string());
                    tokio::select! {
                        biased;
                        _ = stop.changed() => break,
                        _ = shutdown.changed() => break,
                        _ = tokio::time::sleep(Duration::from_millis(50)) => continue,
                    }
                }
            }
        };

        let rules = rules_rx.borrow().clone();
        {
            let mut guard = status.write().expect("OSC inbound status lock");
            guard.received_packets += 1;
            guard.last_received_at = Some(now_iso8601());
        }

        let routed = match route_datagram(&buf[..len], &rules) {
            Ok(routed) => routed,
            Err(error) => {
                tracing::debug!(%error, "Dropped malformed OSC inbound packet");
                status
                    .write()
                    .expect("OSC inbound status lock")
                    .decode_errors += 1;
                continue;
            }
        };

        if let Some(forward_bytes) = routed.passthrough {
            if rules.forward_port == listen_port {
                tracing::warn!(
                    port = listen_port,
                    "Skipped OSC passthrough because target port matches inbound port"
                );
            } else {
                let target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, rules.forward_port);
                match forward_socket.send_to(&forward_bytes, target).await {
                    Ok(_) => {
                        status
                            .write()
                            .expect("OSC inbound status lock")
                            .forwarded_packets += 1;
                    }
                    Err(error) => {
                        tracing::warn!(%error, %target, "OSC inbound passthrough send failed");
                        status.write().expect("OSC inbound status lock").last_error =
                            Some(error.to_string());
                    }
                }
            }
        }

        let now = Instant::now();
        for text in routed.intercepted_chatbox {
            let normalized = compact_text(&text);
            if normalized.is_empty() {
                continue;
            }
            if let Some((prev, at)) = &last_intercepted
                && *prev == normalized
                && now.saturating_duration_since(*at) < DUPLICATE_WINDOW
            {
                continue;
            }
            last_intercepted = Some((normalized, now));
            match chatbox_tx.try_send(text) {
                Ok(()) => {
                    status
                        .write()
                        .expect("OSC inbound status lock")
                        .intercepted_chatbox_messages += 1;
                }
                Err(_) => {
                    tracing::warn!("OSC inbound Chatbox queue is full; dropping message");
                    status
                        .write()
                        .expect("OSC inbound status lock")
                        .dropped_messages += 1;
                }
            }
        }
    }
}

async fn run_chatbox_worker(
    mut receiver: mpsc::Receiver<String>,
    on_chatbox: InboundChatboxCallback,
    status: Arc<RwLock<OscInboundRuntimeStatus>>,
    mut stop: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *stop.borrow() || *shutdown.borrow() {
            break;
        }
        let text = tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = shutdown.changed() => break,
            item = receiver.recv() => match item {
                Some(text) => text,
                None => break,
            }
        };
        let outcome = tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = shutdown.changed() => break,
            result = (on_chatbox)(text) => result,
        };
        let mut guard = status.write().expect("OSC inbound status lock");
        match outcome {
            Ok(()) => {
                if guard.state == "listening" {
                    guard.last_error = None;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "OSC inbound Chatbox processing failed");
                guard.last_error = Some(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(packet: OscPacket) -> Vec<u8> {
        rosc::encoder::encode(&packet).unwrap()
    }

    #[test]
    fn routes_immediate_chatbox_and_passes_through_drafts_clear_and_other_addresses() {
        let rules = OscInboundRules {
            intercept_chatbox_input: true,
            forward_unmatched: true,
            forward_port: 9000,
        };

        let immediate = encode(OscPacket::Message(OscMessage {
            addr: "/chatbox/input".into(),
            args: vec![OscType::String("hello".into()), OscType::Bool(true)],
        }));
        let routed = route_datagram(&immediate, &rules).unwrap();
        assert_eq!(routed.intercepted_chatbox, vec!["hello"]);
        assert!(routed.passthrough.is_none());

        let draft = encode(OscPacket::Message(OscMessage {
            addr: "/chatbox/input".into(),
            args: vec![OscType::String("draft".into()), OscType::Bool(false)],
        }));
        let routed = route_datagram(&draft, &rules).unwrap();
        assert!(routed.intercepted_chatbox.is_empty());
        assert_eq!(routed.passthrough.as_deref(), Some(draft.as_slice()));

        let clear = encode(OscPacket::Message(OscMessage {
            addr: "/chatbox/input".into(),
            args: vec![OscType::String("   ".into()), OscType::Bool(true)],
        }));
        let routed = route_datagram(&clear, &rules).unwrap();
        assert!(routed.intercepted_chatbox.is_empty());
        assert!(routed.passthrough.is_some());

        let typing = encode(OscPacket::Message(OscMessage {
            addr: "/chatbox/typing".into(),
            args: vec![OscType::Bool(true)],
        }));
        let routed = route_datagram(&typing, &rules).unwrap();
        assert!(routed.intercepted_chatbox.is_empty());
        assert_eq!(routed.passthrough.as_deref(), Some(typing.as_slice()));
    }

    #[test]
    fn splits_mixed_bundle_into_intercepted_and_passthrough() {
        let rules = OscInboundRules {
            intercept_chatbox_input: true,
            forward_unmatched: true,
            forward_port: 9000,
        };
        let bundle = encode(OscPacket::Bundle(OscBundle {
            timetag: OscTime {
                seconds: 0,
                fractional: 1,
            },
            content: vec![
                OscPacket::Message(OscMessage {
                    addr: "/chatbox/input".into(),
                    args: vec![OscType::String("hi".into()), OscType::Int(1)],
                }),
                OscPacket::Message(OscMessage {
                    addr: "/avatar/parameters/Test".into(),
                    args: vec![OscType::Bool(true)],
                }),
            ],
        }));

        let routed = route_datagram(&bundle, &rules).unwrap();
        assert_eq!(routed.intercepted_chatbox, vec!["hi"]);
        let (_, forwarded) =
            rosc::decoder::decode_udp(routed.passthrough.as_ref().unwrap()).unwrap();
        match forwarded {
            OscPacket::Message(msg) => assert_eq!(msg.addr, "/avatar/parameters/Test"),
            other => panic!("expected single forwarded message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn listener_intercepts_dedups_and_forwards_over_loopback_udp() {
        let vrchat = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let vrchat_port = vrchat.local_addr().unwrap().port();
        let free_probe = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let inbound_port = free_probe.local_addr().unwrap().port();
        drop(free_probe);

        let mut config = OscConfig {
            port: vrchat_port,
            ..OscConfig::default()
        };
        config.inbound.enabled = true;
        config.inbound.port = inbound_port;

        let (seen_tx, mut seen_rx) = mpsc::channel(8);
        let callback: InboundChatboxCallback = Arc::new(move |text| {
            let tx = seen_tx.clone();
            Box::pin(async move {
                let _ = tx.send(text).await;
                Ok(())
            })
        });
        let status = Arc::new(RwLock::new(OscInboundRuntimeStatus::disabled()));
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let server = OscInboundServer::start(&config, callback, Arc::clone(&status), shutdown_rx)
            .await
            .unwrap()
            .unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let chat = encode(OscPacket::Message(OscMessage {
            addr: "/chatbox/input".into(),
            args: vec![OscType::String("hello vr".into()), OscType::Bool(true)],
        }));
        client.send_to(&chat, server.address).await.unwrap();
        // Immediate duplicate within 1s should be deduplicated.
        client.send_to(&chat, server.address).await.unwrap();

        let typing = encode(OscPacket::Message(OscMessage {
            addr: "/chatbox/typing".into(),
            args: vec![OscType::Bool(true)],
        }));
        client.send_to(&typing, server.address).await.unwrap();

        let received = tokio::time::timeout(Duration::from_secs(2), seen_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, "hello vr");

        let mut buf = [0u8; 256];
        let (len, _) = tokio::time::timeout(Duration::from_secs(2), vrchat.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..len], typing.as_slice());

        assert!(
            tokio::time::timeout(Duration::from_millis(120), seen_rx.recv())
                .await
                .is_err()
        );
        let snapshot = status.read().unwrap().clone();
        assert_eq!(snapshot.state, "listening");
        assert_eq!(snapshot.intercepted_chatbox_messages, 1);
        assert_eq!(snapshot.forwarded_packets, 1);

        server.stop().await;
    }
}
