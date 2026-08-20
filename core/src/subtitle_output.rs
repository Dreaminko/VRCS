use tokio::sync::broadcast;

use crate::chatbox::ChatboxMessage;
use crate::domain_events::DomainEventHub;
use crate::models::{Subtitle, SubtitleTranslation};
use crate::osc::OscChatboxDispatcher;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PresentationEvent {
    RecognitionPartial {
        utterance_id: String,
        source: String,
        text: String,
        language: Option<String>,
    },
    RecognitionCancelled {
        utterance_id: String,
        source: String,
    },
    RecognitionReset {
        source: String,
    },
    Final {
        utterance_id: Option<String>,
        subtitle: Subtitle,
    },
    TranslationPartial {
        subtitle_id: i64,
        text: String,
    },
    TranslationCompleted {
        subtitle_id: i64,
        translation: SubtitleTranslation,
    },
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranslationEvent {
    TranslationStarted {
        subtitle_id: i64,
        target_language: String,
        preferred: bool,
    },
    TranslationPartial {
        subtitle_id: i64,
        text: String,
        target_language: String,
        preferred: bool,
    },
    TranslationCompleted {
        subtitle_id: i64,
        translation: SubtitleTranslation,
        preferred: bool,
    },
    TranslationFailed {
        subtitle_id: i64,
        target_language: String,
        preferred: bool,
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
    presentation: broadcast::Sender<PresentationEvent>,
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
        let (presentation, _) = broadcast::channel(256);
        Self {
            subtitles,
            translations,
            presentation,
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

    pub fn subscribe_presentation_events(&self) -> broadcast::Receiver<PresentationEvent> {
        self.presentation.subscribe()
    }

    pub fn subtitle_stored_with_message(
        &self,
        subtitle: Subtitle,
        wait_for_translation: bool,
        translation_targets: Vec<String>,
        message_id: &str,
    ) {
        self.events.asr_final(message_id, &subtitle);
        self.publish_subtitle(subtitle.clone(), Some(message_id));
        self.osc.publish_subtitle_with_targets(
            subtitle,
            wait_for_translation,
            translation_targets,
            message_id.into(),
        );
    }

    pub fn subtitle_recorded(&self, subtitle: Subtitle) {
        self.publish_subtitle(subtitle, None);
    }

    fn publish_subtitle(&self, subtitle: Subtitle, utterance_id: Option<&str>) {
        let _ = self.subtitles.send(subtitle.clone());
        let _ = self.presentation.send(PresentationEvent::Final {
            utterance_id: utterance_id.map(str::to_owned),
            subtitle,
        });
    }

    pub fn asr_partial(&self, message_id: &str, source: &str, text: &str, language: Option<&str>) {
        self.events.asr_partial(message_id, source, text, language);
        let _ = self
            .presentation
            .send(PresentationEvent::RecognitionPartial {
                utterance_id: message_id.into(),
                source: source.into(),
                text: text.into(),
                language: language.map(str::to_owned),
            });
    }

    pub fn asr_cancelled(&self, message_id: &str, source: &str, reason: &str) {
        self.events.asr_cancelled(message_id, source, reason);
        let _ = self
            .presentation
            .send(PresentationEvent::RecognitionCancelled {
                utterance_id: message_id.into(),
                source: source.into(),
            });
    }

    pub fn asr_reset(&self, source: &str) {
        self.events.asr_reset(source);
        let _ = self.presentation.send(PresentationEvent::RecognitionReset {
            source: source.into(),
        });
    }

    pub fn asr_failed(&self, message_id: Option<&str>, source: &str, code: &str, detail: &str) {
        self.events.asr_failed(message_id, source, code, detail);
        let event = match message_id {
            Some(utterance_id) => PresentationEvent::RecognitionCancelled {
                utterance_id: utterance_id.into(),
                source: source.into(),
            },
            None => PresentationEvent::RecognitionReset {
                source: source.into(),
            },
        };
        let _ = self.presentation.send(event);
    }

    pub fn translation_started_with_message(
        &self,
        subtitle_id: i64,
        target_language: &str,
        preferred: bool,
        message_id: &str,
        source: &str,
    ) {
        self.events
            .translation_started(message_id, source, subtitle_id, target_language);
        let _ = self
            .translations
            .send(TranslationEvent::TranslationStarted {
                subtitle_id,
                target_language: target_language.into(),
                preferred,
            });
    }

    pub fn translation_partial_with_message(
        &self,
        subtitle_id: i64,
        text: String,
        target_language: String,
        preferred: bool,
        message_id: &str,
        source: &str,
    ) {
        self.events
            .translation_partial(message_id, source, subtitle_id, &text, &target_language);
        if preferred {
            let _ = self
                .presentation
                .send(PresentationEvent::TranslationPartial {
                    subtitle_id,
                    text: text.clone(),
                });
        }
        let _ = self
            .translations
            .send(TranslationEvent::TranslationPartial {
                subtitle_id,
                text,
                target_language,
                preferred,
            });
    }

    pub fn translation_completed_with_message(
        &self,
        subtitle_id: i64,
        translation: SubtitleTranslation,
        preferred: bool,
        message_id: &str,
        source: &str,
    ) {
        self.events
            .translation_completed(message_id, source, subtitle_id, &translation);
        self.osc
            .translation_completed(subtitle_id, translation.clone(), preferred);
        if preferred {
            let _ = self
                .presentation
                .send(PresentationEvent::TranslationCompleted {
                    subtitle_id,
                    translation: translation.clone(),
                });
        }
        let _ = self
            .translations
            .send(TranslationEvent::TranslationCompleted {
                subtitle_id,
                translation,
                preferred,
            });
    }

    pub fn translation_failed_with_message(
        &self,
        subtitle_id: i64,
        code: String,
        detail: String,
        target_language: &str,
        preferred: bool,
        message_id: &str,
        source: &str,
    ) {
        self.events.translation_failed(
            message_id,
            source,
            subtitle_id,
            target_language,
            &code,
            &detail,
        );
        self.osc
            .translation_failed(subtitle_id, target_language, preferred);
        let _ = self.translations.send(TranslationEvent::TranslationFailed {
            subtitle_id,
            target_language: target_language.into(),
            preferred,
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

    fn subtitle(id: i64) -> Subtitle {
        Subtitle {
            id: Some(id),
            conversation_id: Some("conversation-test".into()),
            text: "hello".into(),
            language: Some("en".into()),
            started_at: None,
            ended_at: None,
            source: "speaker".into(),
            created_at: now_iso8601(),
            translations: Vec::new(),
        }
    }

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
        output.subtitle_stored_with_message(
            subtitle(7),
            true,
            vec!["zh-Hans".into()],
            "utterance-7",
        );
        output.translation_started_with_message(7, "zh-Hans", true, "utterance-7", "speaker");

        let final_event = receiver.recv().await.unwrap();
        let translation_event = receiver.recv().await.unwrap();
        assert_eq!(final_event.event_type, "asr.final");
        assert_eq!(final_event.payload["subtitle"]["id"], 7);
        assert_eq!(translation_event.event_type, "translation.started");
        assert_eq!(final_event.message_id, "utterance-7");
        assert_eq!(translation_event.message_id, final_event.message_id);
    }

    #[tokio::test]
    async fn presentation_final_and_translation_updates_are_published() {
        let (subtitles, _) = broadcast::channel(4);
        let (translations, _) = broadcast::channel(4);
        let events = DomainEventHub::new();
        let mut domain_receiver = events.subscribe();
        let output = SubtitleLifecyclePublisher::with_domain_events(
            subtitles,
            translations,
            OscChatboxDispatcher::new(OscConfig::default()),
            events,
        );
        let mut receiver = output.subscribe_presentation_events();

        output.asr_partial("utterance-7", "speaker", "hel", Some("en"));
        let partial = domain_receiver.recv().await.unwrap();
        assert_eq!(partial.event_type, "asr.partial");
        assert_eq!(partial.message_id, "utterance-7");
        assert!(matches!(
            receiver.recv().await.unwrap(),
            PresentationEvent::RecognitionPartial {
                utterance_id,
                source,
                text,
                language,
            } if utterance_id == "utterance-7"
                && source == "speaker"
                && text == "hel"
                && language.as_deref() == Some("en")
        ));

        output.subtitle_stored_with_message(
            subtitle(7),
            true,
            vec!["zh-Hans".into()],
            "utterance-7",
        );
        output.translation_partial_with_message(
            7,
            "你".into(),
            "zh-Hans".into(),
            true,
            "utterance-7",
            "speaker",
        );
        let translation = SubtitleTranslation {
            text: "你好".into(),
            source_language: Some("en".into()),
            target_language: "zh-Hans".into(),
            provider: "test".into(),
            model: None,
            created_at: now_iso8601(),
        };
        output.translation_completed_with_message(7, translation, true, "utterance-7", "speaker");

        assert!(matches!(
            receiver.recv().await.unwrap(),
            PresentationEvent::Final { utterance_id, subtitle }
                if utterance_id.as_deref() == Some("utterance-7") && subtitle.id == Some(7)
        ));
        assert!(matches!(
            receiver.recv().await.unwrap(),
            PresentationEvent::TranslationPartial { subtitle_id: 7, .. }
        ));
        assert!(matches!(
            receiver.recv().await.unwrap(),
            PresentationEvent::TranslationCompleted { subtitle_id: 7, .. }
        ));
    }

    #[tokio::test]
    async fn recognition_termination_is_published_for_overlay_cleanup() {
        let (subtitles, _) = broadcast::channel(4);
        let (translations, _) = broadcast::channel(4);
        let output = SubtitleLifecyclePublisher::new(
            subtitles,
            translations,
            OscChatboxDispatcher::new(OscConfig::default()),
        );
        let mut receiver = output.subscribe_presentation_events();

        output.asr_cancelled("utterance-7", "speaker", "filtered");
        output.asr_failed(Some("utterance-8"), "speaker", "asr.failed", "failed");
        output.asr_reset("microphone");
        output.asr_failed(None, "speaker", "asr.disconnected", "disconnected");

        assert!(matches!(
            receiver.recv().await.unwrap(),
            PresentationEvent::RecognitionCancelled { utterance_id, source }
                if utterance_id == "utterance-7" && source == "speaker"
        ));
        assert!(matches!(
            receiver.recv().await.unwrap(),
            PresentationEvent::RecognitionCancelled { utterance_id, source }
                if utterance_id == "utterance-8" && source == "speaker"
        ));
        assert!(matches!(
            receiver.recv().await.unwrap(),
            PresentationEvent::RecognitionReset { source } if source == "microphone"
        ));
        assert!(matches!(
            receiver.recv().await.unwrap(),
            PresentationEvent::RecognitionReset { source } if source == "speaker"
        ));
    }

    #[tokio::test]
    async fn lagged_presentation_receivers_do_not_block_publishers() {
        let (subtitles, _) = broadcast::channel(4);
        let (translations, _) = broadcast::channel(4);
        let output = SubtitleLifecyclePublisher::new(
            subtitles,
            translations,
            OscChatboxDispatcher::new(OscConfig::default()),
        );
        let mut receiver = output.subscribe_presentation_events();

        for index in 0..300 {
            output.subtitle_recorded(subtitle(index));
        }

        assert!(matches!(
            receiver.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
    }
}
