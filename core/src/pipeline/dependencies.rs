use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::asr::AsrService;
use crate::config::{ApiProfile, TranslationConfig};
use crate::db::Database;
use crate::models::{now_iso8601, LiveTranscription, Subtitle};
use crate::subtitle_output::SubtitleLifecyclePublisher;
use crate::translation::TranslationDispatcher;

#[derive(Clone)]
pub(crate) struct PipelineDependencies {
    asr: Arc<Mutex<AsrService>>,
    database: Arc<Mutex<Database>>,
    live: broadcast::Sender<LiveTranscription>,
    history_limit: u32,
    translation: TranslationDispatcher,
    translation_config: TranslationConfig,
    api_profiles: Vec<ApiProfile>,
    output: SubtitleLifecyclePublisher,
}

impl PipelineDependencies {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        asr: Arc<Mutex<AsrService>>,
        database: Arc<Mutex<Database>>,
        live: broadcast::Sender<LiveTranscription>,
        history_limit: u32,
        translation: TranslationDispatcher,
        translation_config: TranslationConfig,
        api_profiles: Vec<ApiProfile>,
        output: SubtitleLifecyclePublisher,
    ) -> Self {
        Self {
            asr,
            database,
            live,
            history_limit,
            translation,
            translation_config,
            api_profiles,
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
            text,
            language,
            started_at: None,
            ended_at: None,
            source: source.into(),
            created_at: now_iso8601(),
            translations: Vec::new(),
        };
        let database = Arc::clone(&self.database);
        let history_limit = self.history_limit;
        let saved = match tokio::task::spawn_blocking(move || {
            database
                .lock()
                .map_err(|_| "Database lock is unavailable".to_string())?
                .add_subtitle(&subtitle, history_limit)
                .map_err(|error| error.to_string())
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
        let translation_settings = automatic_translation_settings(&self.translation_config, source);
        self.output.subtitle_stored_with_message(
            saved.clone(),
            translation_settings.is_some(),
            &message_id,
        );
        if let Some(settings) = translation_settings {
            if let Err(detail) = self.translation.enqueue(
                saved.clone(),
                settings,
                self.api_profiles.clone(),
                message_id.clone(),
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
) -> Option<TranslationConfig> {
    if source == "microphone" {
        if !config.translate_microphone {
            return None;
        }
        let mut settings = config.clone();
        settings.target_language = settings.microphone_target_language.clone();
        return Some(settings);
    }
    (config.mode == "automatic").then(|| config.clone())
}

#[cfg(test)]
mod tests {
    use super::automatic_translation_settings;
    use crate::config::TranslationConfig;

    #[test]
    fn microphone_translation_has_an_independent_switch_and_target() {
        let mut config = TranslationConfig {
            mode: "disabled".into(),
            target_language: "zh-Hans".into(),
            microphone_target_language: "ja".into(),
            ..TranslationConfig::default()
        };

        assert!(automatic_translation_settings(&config, "microphone").is_none());
        config.translate_microphone = true;
        let settings = automatic_translation_settings(&config, "microphone").unwrap();
        assert_eq!(settings.target_language, "ja");
        assert!(automatic_translation_settings(&config, "speaker").is_none());
    }

    #[test]
    fn automatic_mode_still_translates_other_voices() {
        let config = TranslationConfig {
            mode: "automatic".into(),
            target_language: "zh-Hans".into(),
            ..TranslationConfig::default()
        };

        let settings = automatic_translation_settings(&config, "speaker").unwrap();
        assert_eq!(settings.target_language, "zh-Hans");
        assert!(automatic_translation_settings(&config, "microphone").is_none());
    }
}
