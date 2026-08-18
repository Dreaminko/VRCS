use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::asr::AsrService;
use crate::config::{AppConfig, TranslationConfig};
use crate::db::conversations::{publish_latest_catalog, ConversationCatalog};
use crate::db::Database;
use crate::models::{now_iso8601, LiveTranscription, Subtitle};
use crate::subtitle_output::SubtitleLifecyclePublisher;
use crate::translation::{same_translation_language, TranslationDispatcher};

#[derive(Clone)]
pub(crate) struct PipelineDependencies {
    asr: Arc<Mutex<AsrService>>,
    database: Arc<Mutex<Database>>,
    live: broadcast::Sender<LiveTranscription>,
    conversation_catalog: broadcast::Sender<ConversationCatalog>,
    translation: TranslationDispatcher,
    config: Arc<std::sync::RwLock<AppConfig>>,
    output: SubtitleLifecyclePublisher,
}

impl PipelineDependencies {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        asr: Arc<Mutex<AsrService>>,
        database: Arc<Mutex<Database>>,
        live: broadcast::Sender<LiveTranscription>,
        conversation_catalog: broadcast::Sender<ConversationCatalog>,
        translation: TranslationDispatcher,
        config: Arc<std::sync::RwLock<AppConfig>>,
        output: SubtitleLifecyclePublisher,
    ) -> Self {
        Self {
            asr,
            database,
            live,
            conversation_catalog,
            translation,
            config,
            output,
        }
    }

    pub(crate) fn publish_live(&self, event: LiveTranscription) {
        match &event {
            LiveTranscription::Partial {
                utterance_id,
                source,
                text,
                language,
            } => self
                .output
                .asr_partial(utterance_id, source, text, language.as_deref()),
            LiveTranscription::Failed {
                source,
                code,
                detail,
            } => self.output.asr_failed(
                &format!("utterance-{}", uuid::Uuid::new_v4()),
                source,
                code,
                detail,
            ),
            LiveTranscription::AudioLevel { .. } => {}
        }
        let _ = self.live.send(event);
    }

    pub(crate) async fn transcribe_and_publish(
        &self,
        segment: Vec<f32>,
        source: &'static str,
    ) -> Result<(), String> {
        let message_id = format!("utterance-{}", uuid::Uuid::new_v4());
        let rms = (segment.iter().map(|sample| sample * sample).sum::<f32>()
            / segment.len().max(1) as f32)
            .sqrt();
        let peak = segment.iter().copied().map(f32::abs).fold(0.0, f32::max);
        tracing::debug!(
            source,
            samples = segment.len(),
            duration_seconds = segment.len() as f64 / 16_000.0,
            rms,
            peak,
            "sending speech segment to ASR"
        );
        let transcriber = Arc::clone(&self.asr);
        let transcription = match tokio::task::spawn_blocking(move || {
            transcriber.lock().expect("asr lock").transcribe(&segment)
        })
        .await
        {
            Ok(Ok(transcription)) => transcription,
            Ok(Err(detail)) => {
                self.output
                    .asr_failed(&message_id, source, "asr.transcription_failed", &detail);
                return Err(detail);
            }
            Err(error) => {
                let detail = format!("Recognition task exited unexpectedly: {error}");
                self.output
                    .asr_failed(&message_id, source, "asr.task_failed", &detail);
                return Err(detail);
            }
        };
        tracing::debug!(
            source,
            text_length = transcription.text.chars().count(),
            language = transcription.language.as_deref().unwrap_or("unknown"),
            "ASR transcription completed"
        );
        self.publish_text(
            transcription.text,
            transcription.language,
            source,
            message_id,
        )
        .await
    }

    pub(crate) async fn publish_text(
        &self,
        text: String,
        language: Option<String>,
        source: &'static str,
        message_id: String,
    ) -> Result<(), String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }
        let subtitle = Subtitle {
            id: None,
            conversation_id: None,
            text,
            language,
            started_at: None,
            ended_at: None,
            source: source.into(),
            created_at: now_iso8601(),
            translations: Vec::new(),
        };
        let database = Arc::clone(&self.database);
        let conversation_catalog = self.conversation_catalog.clone();
        let saved = match tokio::task::spawn_blocking(move || {
            let database = database
                .lock()
                .map_err(|_| "Database lock is unavailable".to_string())?;
            let saved = database
                .add_subtitle(&subtitle)
                .map_err(|error| error.to_string())?;
            publish_latest_catalog(&database, &conversation_catalog);
            Ok::<_, String>(saved)
        })
        .await
        {
            Ok(Ok(saved)) => saved,
            Ok(Err(detail)) => {
                self.output
                    .asr_failed(&message_id, source, "asr.storage_failed", &detail);
                return Err(detail);
            }
            Err(error) => {
                let detail = format!("Subtitle storage task exited unexpectedly: {error}");
                self.output
                    .asr_failed(&message_id, source, "asr.storage_failed", &detail);
                return Err(detail);
            }
        };
        let (translation_settings, api_profiles, include_vrcx_context) = {
            let config = self.config.read().expect("config lock");
            (
                automatic_translation_settings(
                    &config.translation,
                    source,
                    saved.language.as_deref(),
                ),
                config.asr.api_profiles.clone(),
                config.vrcx.enabled && config.vrcx.include_in_llm_context,
            )
        };
        self.output.subtitle_stored_with_message(
            saved.clone(),
            translation_settings.is_some(),
            &message_id,
        );
        if let Some(settings) = translation_settings {
            if let Err(detail) = self.translation.enqueue(
                saved.clone(),
                settings,
                api_profiles,
                message_id.clone(),
                include_vrcx_context,
            ) {
                if let Some(subtitle_id) = saved.id {
                    self.output.translation_failed_with_message(
                        subtitle_id,
                        "translation.queue_full".into(),
                        detail.clone(),
                        &message_id,
                        source,
                    );
                }
                tracing::warn!(%detail, "automatic translation was not queued");
            }
        }
        Ok(())
    }
}

fn automatic_translation_settings(
    config: &TranslationConfig,
    source: &str,
    source_language: Option<&str>,
) -> Option<TranslationConfig> {
    if config.mode != "automatic" {
        return None;
    }
    let mut settings = config.clone();
    if source == "microphone" {
        settings.target_language = settings.microphone_target_language.clone();
    }
    if source_language
        .is_some_and(|source| same_translation_language(source, &settings.target_language))
    {
        return None;
    }
    Some(settings)
}

#[cfg(test)]
mod tests {
    use super::automatic_translation_settings;
    use crate::config::TranslationConfig;

    #[test]
    fn automatic_mode_translates_microphone_with_its_own_target() {
        let config = TranslationConfig {
            mode: "automatic".into(),
            target_language: "zh-Hans".into(),
            microphone_target_language: "ja".into(),
            ..TranslationConfig::default()
        };

        let microphone = automatic_translation_settings(&config, "microphone", None).unwrap();
        assert_eq!(microphone.target_language, "ja");
        let speaker = automatic_translation_settings(&config, "speaker", None).unwrap();
        assert_eq!(speaker.target_language, "zh-Hans");
    }

    #[test]
    fn automatic_mode_skips_matching_source_and_target_languages() {
        let config = TranslationConfig {
            mode: "automatic".into(),
            target_language: "en".into(),
            microphone_target_language: "ja".into(),
            ..TranslationConfig::default()
        };

        assert!(automatic_translation_settings(&config, "speaker", Some("en-US")).is_none());
        assert!(automatic_translation_settings(&config, "microphone", Some("ja")).is_none());
        assert!(automatic_translation_settings(&config, "speaker", Some("ja")).is_some());
        assert!(automatic_translation_settings(&config, "speaker", None).is_some());
    }

    #[test]
    fn non_automatic_modes_do_not_translate_any_voice_automatically() {
        for mode in ["disabled", "manual"] {
            let config = TranslationConfig {
                mode: mode.into(),
                ..TranslationConfig::default()
            };

            assert!(automatic_translation_settings(&config, "microphone", Some("ja")).is_none());
            assert!(automatic_translation_settings(&config, "speaker", Some("en")).is_none());
        }
    }
}
