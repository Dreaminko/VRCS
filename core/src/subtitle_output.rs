use tokio::sync::broadcast;

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
}

impl SubtitleLifecyclePublisher {
    pub fn new(
        subtitles: broadcast::Sender<Subtitle>,
        translations: broadcast::Sender<TranslationEvent>,
        osc: OscChatboxDispatcher,
    ) -> Self {
        Self {
            subtitles,
            translations,
            osc,
        }
    }

    pub fn subscribe_subtitles(&self) -> broadcast::Receiver<Subtitle> {
        self.subtitles.subscribe()
    }

    pub fn subscribe_translations(&self) -> broadcast::Receiver<TranslationEvent> {
        self.translations.subscribe()
    }

    pub fn subtitle_stored(&self, subtitle: Subtitle, wait_for_translation: bool) {
        let _ = self.subtitles.send(subtitle.clone());
        self.osc.publish_subtitle(subtitle, wait_for_translation);
    }

    pub fn translation_queue_failed(&self, subtitle_id: i64) {
        self.osc.translation_failed(subtitle_id);
    }

    pub fn translation_started(&self, subtitle_id: i64) {
        let _ = self
            .translations
            .send(TranslationEvent::TranslationStarted { subtitle_id });
    }

    pub fn translation_partial(&self, subtitle_id: i64, text: String, target_language: String) {
        let _ = self
            .translations
            .send(TranslationEvent::TranslationPartial {
                subtitle_id,
                text,
                target_language,
            });
    }

    pub fn translation_completed(&self, subtitle_id: i64, translation: SubtitleTranslation) {
        self.osc
            .translation_completed(subtitle_id, translation.clone());
        let _ = self
            .translations
            .send(TranslationEvent::TranslationCompleted {
                subtitle_id,
                translation,
            });
    }

    pub fn translation_failed(&self, subtitle_id: i64, code: String, detail: String) {
        self.osc.translation_failed(subtitle_id);
        let _ = self.translations.send(TranslationEvent::TranslationFailed {
            subtitle_id,
            code,
            detail,
        });
    }
}
