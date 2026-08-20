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
    language_session: Arc<std::sync::RwLock<crate::language_session::ActiveLanguageSession>>,
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
        language_session: Arc<std::sync::RwLock<crate::language_session::ActiveLanguageSession>>,
        output: SubtitleLifecyclePublisher,
    ) -> Self {
        Self {
            asr,
            database,
            live,
            conversation_catalog,
            translation,
            config,
            language_session,
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
                utterance_id,
                source,
                code,
                detail,
            } => self
                .output
                .asr_failed(utterance_id.as_deref(), source, code, detail),
            LiveTranscription::AudioLevel { .. } => {}
        }
        let _ = self.live.send(event);
    }

    pub(crate) fn cancel_recognition(&self, utterance_id: &str, source: &str, reason: &str) {
        self.output.asr_cancelled(utterance_id, source, reason);
    }

    pub(crate) fn reset_recognition(&self, source: &str) {
        self.output.asr_reset(source);
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
                self.output.asr_failed(
                    Some(&message_id),
                    source,
                    "asr.transcription_failed",
                    &detail,
                );
                return Err(detail);
            }
            Err(error) => {
                let detail = format!("Recognition task exited unexpectedly: {error}");
                self.output
                    .asr_failed(Some(&message_id), source, "asr.task_failed", &detail);
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
            self.output.asr_cancelled(&message_id, source, "empty");
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
                    .asr_failed(Some(&message_id), source, "asr.storage_failed", &detail);
                return Err(detail);
            }
            Err(error) => {
                let detail = format!("Subtitle storage task exited unexpectedly: {error}");
                self.output
                    .asr_failed(Some(&message_id), source, "asr.storage_failed", &detail);
                return Err(detail);
            }
        };
        let (translation_targets, translation_prompt, api_profiles, include_vrcx_context) = {
            let config = self.config.read().expect("config lock");
            let language = self
                .language_session
                .read()
                .expect("language session lock")
                .resolve(&config);
            (
                automatic_translation_targets(
                    &language.translation,
                    source,
                    saved.language.as_deref(),
                ),
                language.translation.prompt,
                config.asr.api_profiles.clone(),
                config.vrcx.enabled && config.vrcx.include_in_llm_context,
            )
        };
        self.output.subtitle_stored_with_message(
            saved.clone(),
            translation_targets.is_some(),
            translation_targets
                .as_ref()
                .map(|targets| {
                    targets
                        .iter()
                        .map(|target| target.target_language.clone())
                        .collect()
                })
                .unwrap_or_default(),
            &message_id,
        );
        if let Some(targets) = translation_targets {
            let failed_targets = targets.clone();
            if let Err(detail) = self.translation.enqueue(
                saved.clone(),
                targets,
                translation_prompt,
                api_profiles,
                message_id.clone(),
                include_vrcx_context,
            ) {
                if let Some(subtitle_id) = saved.id {
                    for (index, target) in failed_targets.iter().enumerate() {
                        self.output.translation_failed_with_message(
                            subtitle_id,
                            "translation.queue_full".into(),
                            detail.clone(),
                            &target.target_language,
                            index == 0,
                            &message_id,
                            source,
                        );
                    }
                }
                tracing::warn!(%detail, "automatic translation was not queued");
            }
        }
        Ok(())
    }
}

fn automatic_translation_targets(
    config: &TranslationConfig,
    source: &str,
    source_language: Option<&str>,
) -> Option<Vec<crate::config::TranslationTargetConfig>> {
    if config.mode != "automatic" {
        return None;
    }
    let targets = if source == "microphone" {
        &config.microphone_targets
    } else {
        &config.speaker_targets
    };
    let targets = targets
        .iter()
        .filter(|target| {
            !source_language
                .is_some_and(|source| same_translation_language(source, &target.target_language))
        })
        .cloned()
        .collect::<Vec<_>>();
    (!targets.is_empty()).then_some(targets)
}

#[cfg(test)]
mod tests {
    use super::automatic_translation_targets;
    use crate::config::{TranslationConfig, TranslationTargetConfig};

    #[test]
    fn automatic_mode_translates_microphone_with_its_own_target() {
        let config = TranslationConfig {
            mode: "automatic".into(),
            speaker_targets: vec![TranslationTargetConfig::new("zh-Hans")],
            microphone_targets: vec![TranslationTargetConfig::new("ja")],
            ..TranslationConfig::default()
        };

        let microphone = automatic_translation_targets(&config, "microphone", None).unwrap();
        assert_eq!(microphone[0].target_language, "ja");
        let speaker = automatic_translation_targets(&config, "speaker", None).unwrap();
        assert_eq!(speaker[0].target_language, "zh-Hans");
    }

    #[test]
    fn automatic_mode_skips_matching_source_and_target_languages() {
        let config = TranslationConfig {
            mode: "automatic".into(),
            speaker_targets: vec![TranslationTargetConfig::new("en")],
            microphone_targets: vec![TranslationTargetConfig::new("ja")],
            ..TranslationConfig::default()
        };

        assert!(automatic_translation_targets(&config, "speaker", Some("en-US")).is_none());
        assert!(automatic_translation_targets(&config, "microphone", Some("ja")).is_none());
        assert!(automatic_translation_targets(&config, "speaker", Some("ja")).is_some());
        assert!(automatic_translation_targets(&config, "speaker", None).is_some());
    }

    #[test]
    fn non_automatic_modes_do_not_translate_any_voice_automatically() {
        for mode in ["disabled", "manual"] {
            let config = TranslationConfig {
                mode: mode.into(),
                ..TranslationConfig::default()
            };

            assert!(automatic_translation_targets(&config, "microphone", Some("ja")).is_none());
            assert!(automatic_translation_targets(&config, "speaker", Some("en")).is_none());
        }
    }
}
