use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::chatbox::ChatboxMessage;
use crate::models::{now_iso8601, Subtitle, SubtitleTranslation};

pub const API_VERSION: &str = "1.0";
pub const EVENT_TYPES: [&str; 10] = [
    "asr.partial",
    "asr.final",
    "asr.cancelled",
    "asr.reset",
    "asr.failed",
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
}

impl Default for DomainEventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainEventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.sender.subscribe()
    }

    pub fn asr_partial(&self, message_id: &str, source: &str, text: &str, language: Option<&str>) {
        self.publish(DomainEvent::new(
            "asr.partial",
            message_id,
            source,
            json!({ "text": text, "language": language }),
        ));
    }

    pub fn asr_final(&self, message_id: &str, subtitle: &Subtitle) {
        self.publish(DomainEvent::new(
            "asr.final",
            message_id,
            &subtitle.source,
            json!({
                "subtitle_id": subtitle.id,
                "text": subtitle.text,
                "language": subtitle.language,
                "subtitle": subtitle,
            }),
        ));
    }

    pub fn asr_cancelled(&self, message_id: &str, source: &str, reason: &str) {
        self.publish(DomainEvent::new(
            "asr.cancelled",
            message_id,
            source,
            json!({ "reason": reason }),
        ));
    }

    pub fn asr_reset(&self, source: &str) {
        self.publish(DomainEvent::new("asr.reset", "session", source, json!({})));
    }

    pub fn asr_failed(&self, message_id: Option<&str>, source: &str, code: &str, detail: &str) {
        self.publish(DomainEvent::new(
            "asr.failed",
            message_id.unwrap_or("session"),
            source,
            json!({ "utterance_id": message_id, "code": code, "detail": detail }),
        ));
    }

    pub fn translation_started(
        &self,
        message_id: &str,
        source: &str,
        subtitle_id: i64,
        target_language: &str,
    ) {
        self.publish(DomainEvent::new(
            "translation.started",
            message_id,
            source,
            json!({ "subtitle_id": subtitle_id, "target_language": target_language }),
        ));
    }

    pub fn translation_partial(
        &self,
        message_id: &str,
        source: &str,
        subtitle_id: i64,
        text: &str,
        target_language: &str,
    ) {
        self.publish(DomainEvent::new(
            "translation.partial",
            message_id,
            source,
            json!({
                "subtitle_id": subtitle_id,
                "text": text,
                "target_language": target_language,
            }),
        ));
    }

    pub fn translation_completed(
        &self,
        message_id: &str,
        source: &str,
        subtitle_id: i64,
        translation: &SubtitleTranslation,
    ) {
        self.publish(DomainEvent::new(
            "translation.completed",
            message_id,
            source,
            json!({ "subtitle_id": subtitle_id, "translation": translation }),
        ));
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
        self.publish(DomainEvent::new(
            "translation.failed",
            message_id,
            source,
            json!({
                "subtitle_id": subtitle_id,
                "target_language": target_language,
                "code": code,
                "detail": detail,
            }),
        ));
    }

    pub fn chatbox_sent(&self, message_id: &str, message: &ChatboxMessage) {
        self.publish(DomainEvent::new(
            "chatbox.sent",
            message_id,
            "chatbox",
            json!({ "message": message }),
        ));
    }

    fn publish(&self, event: DomainEvent) {
        let _ = self.sender.send(event);
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
