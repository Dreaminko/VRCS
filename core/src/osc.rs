use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(test)]
use rosc::{OscPacket, OscType};
use serde::Serialize;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};

use crate::config::OscConfig;
use crate::models::{now_iso8601, Subtitle, SubtitleTranslation};

mod message;
mod transport;

use message::format_chatbox;
#[cfg(test)]
use message::CHATBOX_LIMIT;
use transport::send_chatbox;
#[cfg(test)]
use transport::CHATBOX_ADDRESS;
const TRANSLATION_GRACE: Duration = Duration::from_millis(1_200);
const SEND_INTERVAL: Duration = Duration::from_millis(1_500);
const LATE_TRANSLATION_TTL: Duration = Duration::from_secs(30);
const DISPLAY_QUEUE_CAPACITY: usize = 4;
const EVENT_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Serialize)]
pub struct OscRuntimeStatus {
    pub enabled: bool,
    pub target: String,
    pub status: String,
    pub last_error: Option<String>,
    pub last_sent_at: Option<String>,
    pub dropped_messages: u64,
    pub send_gate: String,
}

#[derive(Clone)]
pub struct OscChatboxDispatcher {
    sender: mpsc::Sender<OscEvent>,
    config: watch::Sender<OscConfigState>,
    status: Arc<Mutex<OscRuntimeStatus>>,
}

enum OscEvent {
    Subtitle {
        generation: u64,
        subtitle: Subtitle,
        wait_for_translation: bool,
    },
    TranslationCompleted {
        generation: u64,
        subtitle_id: i64,
        translation: SubtitleTranslation,
    },
    TranslationFailed {
        generation: u64,
        subtitle_id: i64,
    },
    Test {
        generation: u64,
    },
}

impl OscEvent {
    fn generation(&self) -> u64 {
        match self {
            Self::Subtitle { generation, .. }
            | Self::TranslationCompleted { generation, .. }
            | Self::TranslationFailed { generation, .. }
            | Self::Test { generation } => *generation,
        }
    }
}

#[derive(Clone)]
struct OscConfigState {
    config: OscConfig,
    generation: u64,
    send_gate: SendGate,
}

#[derive(Clone, Copy, PartialEq)]
enum SendGate {
    Open,
    VrchatMuted,
    MuteUnknown,
}

impl SendGate {
    fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::VrchatMuted => "blocked_vrchat_muted",
            Self::MuteUnknown => "blocked_mute_unknown",
        }
    }

    fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Open => None,
            Self::VrchatMuted => Some("osc.blocked_vrchat_muted"),
            Self::MuteUnknown => Some("osc.blocked_mute_unknown"),
        }
    }
}

struct PendingMessage {
    subtitle_id: Option<i64>,
    original: String,
    translation: Option<String>,
    ready_at: Instant,
}

struct SentMessage {
    subtitle_id: i64,
    original: String,
    translation: Option<String>,
    sent_at: Instant,
}

impl OscChatboxDispatcher {
    pub fn new(config: OscConfig) -> Self {
        let (sender, receiver) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let status = Arc::new(Mutex::new(runtime_status(&config)));
        let (config, config_rx) = watch::channel(OscConfigState {
            send_gate: initial_gate(&config),
            config,
            generation: 0,
        });
        tokio::spawn(run_worker(receiver, config_rx, Arc::clone(&status)));
        Self {
            sender,
            config,
            status,
        }
    }

    pub fn publish_subtitle(&self, subtitle: Subtitle, wait_for_translation: bool) {
        let Ok(generation) = self.active_generation() else {
            return;
        };
        if subtitle.source != "microphone" {
            return;
        }
        self.try_send(OscEvent::Subtitle {
            generation,
            subtitle,
            wait_for_translation,
        });
    }

    pub fn translation_completed(&self, subtitle_id: i64, translation: SubtitleTranslation) {
        if let Ok(generation) = self.active_generation() {
            self.try_send(OscEvent::TranslationCompleted {
                generation,
                subtitle_id,
                translation,
            });
        }
    }

    pub fn translation_failed(&self, subtitle_id: i64) {
        if let Ok(generation) = self.active_generation() {
            self.try_send(OscEvent::TranslationFailed {
                generation,
                subtitle_id,
            });
        }
    }

    pub fn queue_test(&self) -> Result<(), &'static str> {
        let generation = self.active_generation()?;
        self.sender
            .try_send(OscEvent::Test { generation })
            .map_err(|_| {
                self.record_drop();
                "osc.queue_full"
            })
    }

    pub fn update_config(&self, config: OscConfig) {
        *self.status.lock().expect("OSC status lock") = runtime_status(&config);
        self.config.send_modify(|state| {
            if config.mute_sync_enabled && !state.config.mute_sync_enabled {
                state.send_gate = SendGate::MuteUnknown;
            } else if !config.mute_sync_enabled {
                state.send_gate = SendGate::Open;
            }
            state.config = config;
            state.generation = state.generation.wrapping_add(1);
        });
        self.refresh_gate_status();
    }

    pub fn update_mute_status(&self, muted: Option<bool>) {
        self.config.send_modify(|state| {
            let next = if !state.config.mute_sync_enabled {
                SendGate::Open
            } else {
                match muted {
                    Some(true) => SendGate::VrchatMuted,
                    Some(false) => SendGate::Open,
                    None => SendGate::MuteUnknown,
                }
            };
            if next != state.send_gate {
                state.send_gate = next;
                state.generation = state.generation.wrapping_add(1);
            }
        });
        self.refresh_gate_status();
    }

    pub fn status(&self) -> OscRuntimeStatus {
        self.status.lock().expect("OSC status lock").clone()
    }

    fn active_generation(&self) -> Result<u64, &'static str> {
        let state = self.config.borrow();
        if !state.config.enabled {
            return Err("osc.disabled");
        }
        if let Some(code) = state.send_gate.error_code() {
            return Err(code);
        }
        Ok(state.generation)
    }

    fn refresh_gate_status(&self) {
        let state = self.config.borrow();
        let mut status = self.status.lock().expect("OSC status lock");
        status.send_gate = state.send_gate.label().into();
    }

    fn try_send(&self, event: OscEvent) {
        if self.sender.try_send(event).is_err() {
            self.record_drop();
        }
    }

    fn record_drop(&self) {
        self.status
            .lock()
            .expect("OSC status lock")
            .dropped_messages += 1;
    }
}

async fn run_worker(
    mut receiver: mpsc::Receiver<OscEvent>,
    mut config_rx: watch::Receiver<OscConfigState>,
    status: Arc<Mutex<OscRuntimeStatus>>,
) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await;
    let mut config = config_rx.borrow().clone();
    let mut queue = VecDeque::<PendingMessage>::new();
    let mut latest_subtitle_id = None;
    let mut current_sent = None::<SentMessage>;
    let mut last_send = None::<Instant>;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            changed = config_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                config = config_rx.borrow().clone();
                queue.clear();
                current_sent = None;
                last_send = None;
            }
            event = receiver.recv() => {
                let Some(event) = event else { break };
                if event.generation() != config.generation {
                    continue;
                }
                match event {
                    OscEvent::Test { .. } => push_bounded(
                        &mut queue,
                        PendingMessage {
                            subtitle_id: None,
                            original: "VRCS OSC test".into(),
                            translation: None,
                            ready_at: Instant::now(),
                        },
                        &status,
                    ),
                    OscEvent::Subtitle { subtitle, wait_for_translation, .. } => {
                        let Some(subtitle_id) = subtitle.id else { continue };
                        latest_subtitle_id = Some(subtitle_id);
                        current_sent = None;
                        push_bounded(
                            &mut queue,
                            PendingMessage {
                                subtitle_id: Some(subtitle_id),
                                original: subtitle.text,
                                translation: None,
                                ready_at: Instant::now() + if wait_for_translation {
                                    TRANSLATION_GRACE
                                } else {
                                    Duration::ZERO
                                },
                            },
                            &status,
                        );
                    }
                    OscEvent::TranslationFailed { subtitle_id, .. } => {
                        if let Some(message) = queue.iter_mut().find(|item| item.subtitle_id == Some(subtitle_id)) {
                            message.ready_at = Instant::now();
                        }
                    }
                    OscEvent::TranslationCompleted { subtitle_id, translation, .. } => {
                        if let Some(message) = queue.iter_mut().find(|item| item.subtitle_id == Some(subtitle_id)) {
                            message.translation = Some(translation.text);
                            message.ready_at = Instant::now();
                        } else if latest_subtitle_id == Some(subtitle_id) {
                            if let Some(sent) = current_sent.as_ref().filter(|sent| {
                                sent.subtitle_id == subtitle_id
                                    && sent.translation.is_none()
                                    && sent.sent_at.elapsed() <= LATE_TRANSLATION_TTL
                            }) {
                                push_bounded(
                                    &mut queue,
                                    PendingMessage {
                                        subtitle_id: Some(subtitle_id),
                                        original: sent.original.clone(),
                                        translation: Some(translation.text),
                                        ready_at: Instant::now(),
                                    },
                                    &status,
                                );
                            }
                        }
                    }
                }
            }
            _ = tick.tick() => {
                if !config.config.enabled || config.send_gate != SendGate::Open {
                    queue.clear();
                    continue;
                }
                let ready = queue.front().is_some_and(|message| message.ready_at <= Instant::now());
                let rate_ready = last_send.is_none_or(|sent| sent.elapsed() >= SEND_INTERVAL);
                if !ready || !rate_ready {
                    continue;
                }
                let Some(message) = queue.pop_front() else { continue };
                let text = format_chatbox(&message.original, message.translation.as_deref());
                let port = config.config.port;
                let result = match &socket {
                    Ok(socket) => {
                        tokio::select! {
                            biased;
                            changed = config_rx.changed() => {
                                if changed.is_err() {
                                    break;
                                }
                                config = config_rx.borrow().clone();
                                queue.clear();
                                current_sent = None;
                                last_send = None;
                                continue;
                            }
                            result = send_chatbox(socket, port, &text) => result,
                        }
                    }
                    Err(error) => Err(error.to_string()),
                };
                let mut runtime = status.lock().expect("OSC status lock");
                match result {
                    Ok(()) => {
                        runtime.status = "ready".into();
                        runtime.last_error = None;
                        runtime.last_sent_at = Some(now_iso8601());
                        last_send = Some(Instant::now());
                        if let Some(subtitle_id) = message.subtitle_id {
                            current_sent = Some(SentMessage {
                                subtitle_id,
                                original: message.original,
                                translation: message.translation,
                                sent_at: Instant::now(),
                            });
                        }
                    }
                    Err(error) => {
                        runtime.status = "error".into();
                        runtime.last_error = Some(error);
                    }
                }
            }
        }
    }
}

fn push_bounded(
    queue: &mut VecDeque<PendingMessage>,
    message: PendingMessage,
    status: &Arc<Mutex<OscRuntimeStatus>>,
) {
    if queue.len() == DISPLAY_QUEUE_CAPACITY {
        queue.pop_front();
        status.lock().expect("OSC status lock").dropped_messages += 1;
    }
    queue.push_back(message);
}

fn runtime_status(config: &OscConfig) -> OscRuntimeStatus {
    OscRuntimeStatus {
        enabled: config.enabled,
        target: format!("127.0.0.1:{}", config.port),
        status: if config.enabled { "ready" } else { "disabled" }.into(),
        last_error: None,
        last_sent_at: None,
        dropped_messages: 0,
        send_gate: initial_gate(config).label().into(),
    }
}

fn initial_gate(config: &OscConfig) -> SendGate {
    if config.mute_sync_enabled {
        SendGate::MuteUnknown
    } else {
        SendGate::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subtitle(id: i64, source: &str, text: &str) -> Subtitle {
        Subtitle {
            id: Some(id),
            text: text.into(),
            language: Some("ja".into()),
            started_at: None,
            ended_at: None,
            source: source.into(),
            created_at: now_iso8601(),
            translations: Vec::new(),
        }
    }

    #[test]
    fn formats_bilingual_messages_within_vrchat_limit() {
        let formatted = format_chatbox(&"原".repeat(100), Some(&"訳".repeat(100)));
        let mut lines = formatted.lines();
        assert_eq!(lines.next().unwrap().chars().count(), 71);
        assert_eq!(lines.next().unwrap().chars().count(), 72);
        assert_eq!(formatted.chars().count(), CHATBOX_LIMIT);
    }

    #[test]
    fn removes_control_characters_and_avoids_duplicate_translation() {
        assert_eq!(
            format_chatbox(" hello\0\nworld ", Some("hello world")),
            "hello world"
        );
    }

    #[tokio::test]
    async fn sends_utf8_chatbox_packet_to_configured_udp_port() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = receiver.local_addr().unwrap().port();
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        send_chatbox(&sender, port, "测试").await.unwrap();

        let mut buffer = [0u8; 512];
        let (length, _) = receiver.recv_from(&mut buffer).await.unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buffer[..length]).unwrap();
        let OscPacket::Message(message) = packet else {
            panic!("expected OSC message")
        };
        assert_eq!(message.addr, CHATBOX_ADDRESS);
        assert_eq!(message.args[0], OscType::String("测试".into()));
        assert_eq!(message.args[1], OscType::Bool(true));
        assert_eq!(message.args[2], OscType::Bool(false));
    }

    #[tokio::test]
    async fn dispatcher_ignores_speaker_and_merges_fast_translation() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let dispatcher = OscChatboxDispatcher::new(OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: false,
            mute_status_toast_enabled: false,
        });
        dispatcher.publish_subtitle(subtitle(1, "speaker", "ignored"), false);
        assert!(tokio::time::timeout(Duration::from_millis(150), async {
            let mut buffer = [0u8; 512];
            receiver.recv_from(&mut buffer).await
        })
        .await
        .is_err());

        dispatcher.publish_subtitle(subtitle(2, "microphone", "こんにちは"), true);
        dispatcher.translation_completed(
            2,
            SubtitleTranslation {
                text: "你好".into(),
                source_language: Some("ja".into()),
                target_language: "zh-Hans".into(),
                provider: "local".into(),
                model: None,
                created_at: now_iso8601(),
            },
        );

        let mut buffer = [0u8; 512];
        let (length, _) =
            tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buffer))
                .await
                .unwrap()
                .unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buffer[..length]).unwrap();
        let OscPacket::Message(message) = packet else {
            panic!("expected OSC message")
        };
        assert_eq!(message.args[0], OscType::String("こんにちは\n你好".into()));
    }

    #[tokio::test]
    async fn config_change_discards_events_from_a_saturated_previous_generation() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let config = OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: false,
            mute_status_toast_enabled: false,
        };
        let dispatcher = OscChatboxDispatcher::new(config.clone());

        // A current-thread Tokio test does not run the worker until this task yields,
        // so this fills the entire data channel before the configuration update.
        for id in 0..EVENT_QUEUE_CAPACITY as i64 {
            dispatcher.publish_subtitle(subtitle(id, "microphone", "stale"), false);
        }
        dispatcher.update_config(config);

        let received = tokio::time::timeout(Duration::from_millis(1_700), async {
            let mut buffer = [0u8; 512];
            receiver.recv_from(&mut buffer).await
        })
        .await;
        assert!(
            received.is_err(),
            "old-generation messages must be discarded"
        );
    }

    #[tokio::test]
    async fn mute_sync_fails_closed_and_clears_queued_messages() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let dispatcher = OscChatboxDispatcher::new(OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: true,
            mute_status_toast_enabled: false,
        });

        assert_eq!(dispatcher.queue_test(), Err("osc.blocked_mute_unknown"));
        dispatcher.update_mute_status(Some(false));
        dispatcher.publish_subtitle(subtitle(1, "microphone", "stale"), false);
        dispatcher.update_mute_status(Some(true));
        assert_eq!(dispatcher.queue_test(), Err("osc.blocked_vrchat_muted"));
        assert_eq!(dispatcher.status().send_gate, "blocked_vrchat_muted");

        let received = tokio::time::timeout(Duration::from_millis(250), async {
            let mut buffer = [0u8; 512];
            receiver.recv_from(&mut buffer).await
        })
        .await;
        assert!(received.is_err(), "muting must discard queued messages");
    }
}
