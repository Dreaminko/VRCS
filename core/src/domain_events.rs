use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::chatbox::ChatboxMessage;
use crate::models::{Subtitle, SubtitleTranslation, now_iso8601};

pub const API_VERSION: &str = "1.0";
pub const EVENT_TYPES: [&str; 12] = [
    "asr.speaker_updated",
    "asr.partial",
    "asr.final",
    "asr.cancelled",
    "asr.reset",
    "asr.failed",
    "translation.live_updated",
    "translation.started",
    "translation.partial",
    "translation.completed",
    "translation.failed",
    "chatbox.sent",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DomainEvent {
    pub api_version: &'static str,
    pub event_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: String,
    pub message_id: String,
    pub source: String,
    pub payload: Value,
}

impl DomainEvent {
    pub fn control(event_type: &str, connection_id: &str, payload: Value) -> Self {
        Self::new(event_type, connection_id, "system", payload)
    }

    fn new(event_type: &str, message_id: &str, source: &str, payload: Value) -> Self {
        Self {
            api_version: API_VERSION,
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.into(),
            timestamp: now_iso8601(),
            message_id: message_id.into(),
            source: source.into(),
            payload,
        }
    }
}

#[derive(Clone)]
pub struct DomainEventHub {
    sender: broadcast::Sender<DomainEvent>,
    ui_sender: broadcast::Sender<DomainEvent>,
}

impl Default for DomainEventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainEventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        let (ui_sender, _) = broadcast::channel(256);
        Self { sender, ui_sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.sender.subscribe()
    }

    pub fn subscribe_ui(&self) -> broadcast::Receiver<DomainEvent> {
        self.ui_sender.subscribe()
    }

    pub fn live_translation(&self, source: &str, snapshot: &crate::models::LiveTranslation) {
        self.publish_ui_and_external(|| {
            DomainEvent::new(
                "translation.live_updated",
                &snapshot.utterance_id,
                source,
                json!(snapshot),
            )
        });
    }

    pub fn asr_partial(&self, message_id: &str, source: &str, text: &str, language: Option<&str>) {
        self.publish_ui_and_external(|| {
            DomainEvent::new(
                "asr.partial",
                message_id,
                source,
                json!({ "text": text, "language": language }),
            )
        });
    }

    #[cfg(any(windows, target_os = "linux", test))]
    pub fn speaker_updated(&self, speaker: &crate::models::SpeakerIdentity) {
        self.publish_ui_and_external(|| {
            DomainEvent::new(
                "asr.speaker_updated",
                &speaker.id,
                "speaker",
                json!({ "speaker": speaker }),
            )
        });
    }

    pub fn asr_final(&self, message_id: &str, subtitle: &Subtitle) {
        self.publish_ui_and_external(|| {
            DomainEvent::new(
                "asr.final",
                message_id,
                &subtitle.source,
                json!({
                    "subtitle_id": subtitle.id,
                    "text": subtitle.text,
                    "language": subtitle.language,
                    "subtitle": subtitle,
                }),
            )
        });
    }

    pub fn asr_player_partial(
        &self,
        message_id: &str,
        source: &str,
        text: &str,
        language: Option<&str>,
        speaker: &crate::models::SpeakerIdentity,
    ) {
        self.publish_ui_and_external(|| {
            DomainEvent::new(
                "asr.partial",
                message_id,
                source,
                json!({ "text": text, "language": language, "speaker": speaker }),
            )
        });
    }

    pub fn asr_cancelled(&self, message_id: &str, source: &str, reason: &str) {
        self.publish_ui_and_external(|| {
            DomainEvent::new(
                "asr.cancelled",
                message_id,
                source,
                json!({ "reason": reason }),
            )
        });
    }

    pub fn asr_reset(&self, source: &str) {
        self.publish_ui_and_external(|| {
            DomainEvent::new("asr.reset", "session", source, json!({}))
        });
    }

    pub fn asr_failed(&self, message_id: Option<&str>, source: &str, code: &str, detail: &str) {
        self.publish_ui_and_external(|| {
            DomainEvent::new(
                "asr.failed",
                message_id.unwrap_or("session"),
                source,
                json!({ "utterance_id": message_id, "code": code, "detail": detail }),
            )
        });
    }

    pub fn translation_started(
        &self,
        message_id: &str,
        source: &str,
        subtitle_id: i64,
        target_language: &str,
    ) {
        self.publish_external(|| {
            DomainEvent::new(
                "translation.started",
                message_id,
                source,
                json!({ "subtitle_id": subtitle_id, "target_language": target_language }),
            )
        });
    }

    pub fn translation_partial(
        &self,
        message_id: &str,
        source: &str,
        subtitle_id: i64,
        text: &str,
        target_language: &str,
    ) {
        self.publish_external(|| {
            DomainEvent::new(
                "translation.partial",
                message_id,
                source,
                json!({
                    "subtitle_id": subtitle_id,
                    "text": text,
                    "target_language": target_language,
                }),
            )
        });
    }

    pub fn translation_completed(
        &self,
        message_id: &str,
        source: &str,
        subtitle_id: i64,
        translation: &SubtitleTranslation,
    ) {
        self.publish_external(|| {
            DomainEvent::new(
                "translation.completed",
                message_id,
                source,
                json!({ "subtitle_id": subtitle_id, "translation": translation }),
            )
        });
    }

    pub fn translation_failed(
        &self,
        message_id: &str,
        source: &str,
        subtitle_id: i64,
        target_language: &str,
        code: &str,
        detail: &str,
    ) {
        self.publish_external(|| {
            DomainEvent::new(
                "translation.failed",
                message_id,
                source,
                json!({
                    "subtitle_id": subtitle_id,
                    "target_language": target_language,
                    "code": code,
                    "detail": detail,
                }),
            )
        });
    }

    pub fn chatbox_sent(&self, message_id: &str, message: &ChatboxMessage) {
        self.publish_external(|| {
            DomainEvent::new(
                "chatbox.sent",
                message_id,
                "chatbox",
                json!({ "message": message }),
            )
        });
    }

    fn publish_external(&self, build: impl FnOnce() -> DomainEvent) {
        if self.sender.receiver_count() > 0 {
            let _ = self.sender.send(build());
        }
    }

    fn publish_ui_and_external(&self, build: impl FnOnce() -> DomainEvent) {
        let external = self.sender.receiver_count() > 0;
        let ui = self.ui_sender.receiver_count() > 0;
        match (external, ui) {
            (false, false) => {}
            (true, false) => {
                let _ = self.sender.send(build());
            }
            (false, true) => {
                let _ = self.ui_sender.send(build());
            }
            (true, true) => {
                let event = build();
                let _ = self.ui_sender.send(event.clone());
                let _ = self.sender.send(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes_have_stable_version_unique_ids_and_utc_timestamps() {
        let first = DomainEvent::new("asr.partial", "utterance-1", "speaker", json!({}));
        let second = DomainEvent::new("asr.partial", "utterance-1", "speaker", json!({}));

        assert_eq!(first.api_version, "1.0");
        assert_ne!(first.event_id, second.event_id);
        assert!(first.timestamp.ends_with('Z'));
        assert_eq!(first.message_id, second.message_id);
    }
}
