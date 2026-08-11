use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use rosc::{encoder, OscMessage, OscPacket, OscType};
use serde::Serialize;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::config::OscConfig;
use crate::models::{now_iso8601, Subtitle, SubtitleTranslation};

const CHATBOX_ADDRESS: &str = "/chatbox/input";
const CHATBOX_LIMIT: usize = 144;
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
}

#[derive(Clone)]
pub struct OscChatboxDispatcher {
    sender: mpsc::Sender<OscEvent>,
    config: Arc<RwLock<OscConfig>>,
    status: Arc<Mutex<OscRuntimeStatus>>,
}

enum OscEvent {
    Subtitle {
        subtitle: Subtitle,
        wait_for_translation: bool,
    },
    TranslationCompleted {
        subtitle_id: i64,
        translation: SubtitleTranslation,
    },
    TranslationFailed {
        subtitle_id: i64,
    },
    Test,
    ConfigChanged,
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
        let config = Arc::new(RwLock::new(config));
        let initial = config.read().expect("OSC config lock").clone();
        let status = Arc::new(Mutex::new(runtime_status(&initial)));
        tokio::spawn(run_worker(
            receiver,
            Arc::clone(&config),
            Arc::clone(&status),
        ));
        Self {
            sender,
            config,
            status,
        }
    }

    pub fn publish_subtitle(&self, subtitle: Subtitle, wait_for_translation: bool) {
        if subtitle.source != "microphone" || !self.enabled() {
            return;
        }
        self.try_send(OscEvent::Subtitle {
            subtitle,
            wait_for_translation,
        });
    }

    pub fn translation_completed(&self, subtitle_id: i64, translation: SubtitleTranslation) {
        if self.enabled() {
            self.try_send(OscEvent::TranslationCompleted {
                subtitle_id,
                translation,
            });
        }
    }

    pub fn translation_failed(&self, subtitle_id: i64) {
        if self.enabled() {
            self.try_send(OscEvent::TranslationFailed { subtitle_id });
        }
    }

    pub fn queue_test(&self) -> Result<(), &'static str> {
        if !self.enabled() {
            return Err("osc.disabled");
        }
        self.sender.try_send(OscEvent::Test).map_err(|_| {
            self.record_drop();
            "osc.queue_full"
        })
    }

    pub fn update_config(&self, config: OscConfig) {
        *self.config.write().expect("OSC config lock") = config.clone();
        *self.status.lock().expect("OSC status lock") = runtime_status(&config);
        self.try_send(OscEvent::ConfigChanged);
    }

    pub fn status(&self) -> OscRuntimeStatus {
        self.status.lock().expect("OSC status lock").clone()
    }

    fn enabled(&self) -> bool {
        self.config.read().expect("OSC config lock").enabled
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
    config: Arc<RwLock<OscConfig>>,
    status: Arc<Mutex<OscRuntimeStatus>>,
) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await;
    let mut queue = VecDeque::<PendingMessage>::new();
    let mut latest_subtitle_id = None;
    let mut current_sent = None::<SentMessage>;
    let mut last_send = None::<Instant>;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            event = receiver.recv() => {
                let Some(event) = event else { break };
                match event {
                    OscEvent::ConfigChanged => {
                        queue.clear();
                        current_sent = None;
                        last_send = None;
                    }
                    OscEvent::Test => push_bounded(
                        &mut queue,
                        PendingMessage {
                            subtitle_id: None,
                            original: "VRCS OSC test".into(),
                            translation: None,
                            ready_at: Instant::now(),
                        },
                        &status,
                    ),
                    OscEvent::Subtitle { subtitle, wait_for_translation } => {
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
                    OscEvent::TranslationFailed { subtitle_id } => {
                        if let Some(message) = queue.iter_mut().find(|item| item.subtitle_id == Some(subtitle_id)) {
                            message.ready_at = Instant::now();
                        }
                    }
                    OscEvent::TranslationCompleted { subtitle_id, translation } => {
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
                let enabled = config.read().expect("OSC config lock").enabled;
                if !enabled {
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
                let port = config.read().expect("OSC config lock").port;
                let result = match &socket {
                    Ok(socket) => send_chatbox(socket, port, &text).await,
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

async fn send_chatbox(socket: &UdpSocket, port: u16, text: &str) -> Result<(), String> {
    let packet = OscPacket::Message(OscMessage {
        addr: CHATBOX_ADDRESS.into(),
        args: vec![
            OscType::String(text.into()),
            OscType::Bool(true),
            OscType::Bool(false),
        ],
    });
    let bytes = encoder::encode(&packet).map_err(|error| error.to_string())?;
    socket
        .send_to(&bytes, SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn runtime_status(config: &OscConfig) -> OscRuntimeStatus {
    OscRuntimeStatus {
        enabled: config.enabled,
        target: format!("127.0.0.1:{}", config.port),
        status: if config.enabled { "ready" } else { "disabled" }.into(),
        last_error: None,
        last_sent_at: None,
        dropped_messages: 0,
    }
}

fn format_chatbox(original: &str, translation: Option<&str>) -> String {
    let original = compact_line(original);
    let translation = translation
        .map(compact_line)
        .filter(|value| value != &original);
    match translation {
        None => truncate(&original, CHATBOX_LIMIT),
        Some(translation) => {
            let combined = format!("{original}\n{translation}");
            if combined.chars().count() <= CHATBOX_LIMIT {
                combined
            } else {
                format!(
                    "{}\n{}",
                    truncate(&original, 71),
                    truncate(&translation, 72)
                )
            }
        }
    }
}

fn compact_line(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_owned();
    }
    value
        .chars()
        .take(limit.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
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
}
