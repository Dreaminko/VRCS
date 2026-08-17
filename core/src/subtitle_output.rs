use tokio::sync::broadcast;

use crate::chatbox::ChatboxMessage;
use crate::domain_events::DomainEventHub;
use crate::models::{Subtitle, SubtitleTranslation};
use crate::osc::OscChatboxDispatcher;

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranslationEvent {
    TranslationStarted {
        subtitle_id: i64,
    },
    TranslationPartial {
        subtitle_id: i64,
        text: String,
        target_language: String,
    },
    TranslationCompleted {
        subtitle_id: i64,
        translation: SubtitleTranslation,
    },
    TranslationFailed {
        subtitle_id: i64,
        code: String,
        detail: String,
    },
}

/// Owns the fan-out from subtitle lifecycle events to UI broadcasts and the
/// optional VRChat OSC output. Recognition and translation workflows report
/// domain outcomes here instead of depending on a concrete output protocol.
#[derive(Clone)]
pub struct SubtitleLifecyclePublisher {
    subtitles: broadcast::Sender<Subtitle>,
    translations: broadcast::Sender<TranslationEvent>,
    osc: OscChatboxDispatcher,
    events: DomainEventHub,
}

impl SubtitleLifecyclePublisher {
    #[cfg(test)]
    pub fn new(
        subtitles: broadcast::Sender<Subtitle>,
        translations: broadcast::Sender<TranslationEvent>,
        osc: OscChatboxDispatcher,
    ) -> Self {
        Self::with_domain_events(subtitles, translations, osc, DomainEventHub::new())
    }

    pub fn with_domain_events(
        subtitles: broadcast::Sender<Subtitle>,
        translations: broadcast::Sender<TranslationEvent>,
        osc: OscChatboxDispatcher,
        events: DomainEventHub,
    ) -> Self {
        Self {
            subtitles,
            translations,
            osc,
            events,
        }
    }

    pub fn subscribe_subtitles(&self) -> broadcast::Receiver<Subtitle> {
        self.subtitles.subscribe()
    }

    pub fn subscribe_translations(&self) -> broadcast::Receiver<TranslationEvent> {
        self.translations.subscribe()
    }

    pub fn subtitle_stored_with_message(
        &self,
        subtitle: Subtitle,
        wait_for_translation: bool,
        message_id: &str,
    ) {
        self.events.asr_final(message_id, &subtitle);
        self.subtitle_recorded(subtitle.clone());
        self.osc.publish_subtitle_with_message_id(
            subtitle,
            wait_for_translation,
            message_id.into(),
        );
    }

    pub fn subtitle_recorded(&self, subtitle: Subtitle) {
        let _ = self.subtitles.send(subtitle);
    }

    pub fn asr_partial(&self, message_id: &str, source: &str, text: &str, language: Option<&str>) {
        self.events.asr_partial(message_id, source, text, language);
    }

    pub fn asr_failed(&self, message_id: &str, source: &str, code: &str, detail: &str) {
        self.events.asr_failed(message_id, source, code, detail);
    }

    pub fn translation_started_with_message(
        &self,
        subtitle_id: i64,
        message_id: &str,
        source: &str,
    ) {
        self.events
            .translation_started(message_id, source, subtitle_id);
        let _ = self
            .translations
            .send(TranslationEvent::TranslationStarted { subtitle_id });
    }

    pub fn translation_partial_with_message(
        &self,
        subtitle_id: i64,
        text: String,
        target_language: String,
        message_id: &str,
        source: &str,
    ) {
        self.events
            .translation_partial(message_id, source, subtitle_id, &text, &target_language);
        let _ = self
            .translations
            .send(TranslationEvent::TranslationPartial {
                subtitle_id,
                text,
                target_language,
            });
    }

    pub fn translation_completed_with_message(
        &self,
        subtitle_id: i64,
        translation: SubtitleTranslation,
        message_id: &str,
        source: &str,
    ) {
        self.events
            .translation_completed(message_id, source, subtitle_id, &translation);
        self.osc
            .translation_completed(subtitle_id, translation.clone());
        let _ = self
            .translations
            .send(TranslationEvent::TranslationCompleted {
                subtitle_id,
                translation,
            });
    }

    pub fn translation_failed_with_message(
        &self,
        subtitle_id: i64,
        code: String,
        detail: String,
        message_id: &str,
        source: &str,
    ) {
        self.events
            .translation_failed(message_id, source, subtitle_id, &code, &detail);
        self.osc.translation_failed(subtitle_id);
        let _ = self.translations.send(TranslationEvent::TranslationFailed {
            subtitle_id,
            code,
            detail,
        });
    }

    pub fn chatbox_sent(&self, message_id: &str, message: &ChatboxMessage) {
        self.events.chatbox_sent(message_id, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OscConfig;
    use crate::models::now_iso8601;

    #[tokio::test]
    async fn recognition_and_automatic_translation_share_the_message_id() {
        let (subtitles, _) = broadcast::channel(4);
        let (translations, _) = broadcast::channel(4);
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        let output = SubtitleLifecyclePublisher::with_domain_events(
            subtitles,
            translations,
            OscChatboxDispatcher::new(OscConfig::default()),
            events,
        );
        let subtitle = Subtitle {
            id: Some(7),
            conversation_id: Some("conversation-test".into()),
            text: "hello".into(),
            language: Some("en".into()),
            started_at: None,
            ended_at: None,
            source: "speaker".into(),
            created_at: now_iso8601(),
            translations: Vec::new(),
        };

        output.subtitle_stored_with_message(subtitle, true, "utterance-7");
        output.translation_started_with_message(7, "utterance-7", "speaker");

        let final_event = receiver.recv().await.unwrap();
        let translation_event = receiver.recv().await.unwrap();
        assert_eq!(final_event.event_type, "asr.final");
        assert_eq!(translation_event.event_type, "translation.started");
        assert_eq!(final_event.message_id, "utterance-7");
        assert_eq!(translation_event.message_id, final_event.message_id);
    }
}
