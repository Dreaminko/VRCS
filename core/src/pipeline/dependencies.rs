use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::asr::AsrService;
use crate::config::{AppConfig, TranslationConfig};
use crate::db::conversations::{publish_latest_catalog, ConversationCatalog};
use crate::db::Database;
use crate::models::{
    now_iso8601, LiveTranscription, Subtitle, SubtitleTranslation, TranslationSourceGroup,
};
use crate::subtitle_output::{SubtitleLifecyclePublisher, TranslationFailure};
use crate::translation::{same_translation_language, TranslationDispatcher};

#[derive(Clone)]
struct PendingNative {
    subtitle_id: i64,
    source: String,
    target_language: String,
    completed: Option<SubtitleTranslation>,
}

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
    pending_native: Arc<Mutex<HashMap<String, PendingNative>>>,
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
            pending_native: Arc::default(),
        }
    }

    pub(crate) fn publish_live_translation(
        &self,
        source: &str,
        snapshot: crate::models::LiveTranslation,
    ) {
        self.output.live_translation(source, snapshot);
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
        if let LiveTranscription::Failed { source, .. } = &event {
            self.fail_pending_native(source);
        }
        let _ = self.live.send(event);
    }

    pub(crate) fn cancel_recognition(&self, utterance_id: &str, source: &str, reason: &str) {
        self.output.asr_cancelled(utterance_id, source, reason);
    }

    pub(crate) fn reset_recognition(&self, source: &str) {
        self.fail_pending_native(source);
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
        self.publish_text_with_translation(text, language, source, message_id, None, false)
            .await
    }

    pub(crate) async fn publish_native_translation(
        &self,
        source: &'static str,
        result: crate::asr::LiveTranslationResult,
    ) -> Result<(), String> {
        let transcript = result.transcript;
        let translation = SubtitleTranslation {
            source_group: None,
            text: transcript.translation.trim().to_owned(),
            source_language: transcript.language.clone(),
            target_language: transcript.target_language,
            provider: result.provider,
            model: Some(result.model),
            created_at: now_iso8601(),
        };
        self.publish_text_with_translation(
            transcript.text,
            transcript.language,
            source,
            transcript.utterance_id,
            Some(translation),
            result.pending,
        )
        .await
    }

    fn fail_pending_native(&self, source: &str) {
        let mut pending = self.pending_native.lock().expect("native translation lock");
        pending.retain(|id, item| {
            if item.source != source {
                return true;
            }
            if item.completed.is_some() {
                return false;
            }
            self.output
                .translation_failed_with_message(TranslationFailure {
                    subtitle_id: item.subtitle_id,
                    code: "translation.live_incomplete".into(),
                    detail: "Live translation ended before the translated text arrived".into(),
                    target_language: &item.target_language,
                    preferred: true,
                    message_id: id,
                    source,
                });
            false
        });
    }

    pub(crate) async fn publish_native_translation_update(
        &self,
        source: &'static str,
        result: crate::asr::LiveTranslationResult,
    ) -> Result<(), String> {
        if result.source_utterance_ids.len() > 1 {
            return self.publish_native_translation_group(source, result).await;
        }
        let transcript = result.transcript;
        let message_id = &transcript.utterance_id;
        let item = self
            .pending_native
            .lock()
            .expect("native translation lock")
            .get(message_id)
            .filter(|item| item.source == source)
            .cloned();
        let Some(item) = item else {
            return Ok(());
        };
        if result.pending {
            if item.completed.is_none() {
                if transcript.translation.trim().is_empty() {
                    self.output.translation_started_with_message(
                        item.subtitle_id,
                        &item.target_language,
                        true,
                        message_id,
                        source,
                    );
                } else {
                    self.output.translation_partial_with_message(
                        item.subtitle_id,
                        transcript.translation,
                        item.target_language,
                        true,
                        message_id,
                        source,
                    );
                }
            }
            return Ok(());
        }
        let translation = SubtitleTranslation {
            text: transcript.translation.trim().to_owned(),
            source_language: transcript.language,
            target_language: item.target_language.clone(),
            provider: result.provider,
            model: Some(result.model),
            created_at: now_iso8601(),
            source_group: None,
        };
        if item
            .completed
            .as_ref()
            .is_some_and(|previous| previous.text == translation.text)
        {
            return Ok(());
        }
        let storage_error = if translation.text.is_empty() {
            None
        } else {
            let database = Arc::clone(&self.database);
            let record = translation.clone();
            let catalog = self.conversation_catalog.clone();
            match tokio::task::spawn_blocking(move || {
                let database = database
                    .lock()
                    .map_err(|_| "Database lock is unavailable".to_string())?;
                let changed = database
                    .save_translation(item.subtitle_id, &record)
                    .map_err(|error| error.to_string())?;
                if changed {
                    publish_latest_catalog(&database, &catalog);
                }
                Ok::<_, String>(())
            })
            .await
            {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(error.to_string()),
            }
        };
        if translation.text.is_empty() || storage_error.is_some() {
            self.pending_native
                .lock()
                .expect("native translation lock")
                .remove(message_id);
            self.output
                .translation_failed_with_message(TranslationFailure {
                    subtitle_id: item.subtitle_id,
                    code: if storage_error.is_some() {
                        "translation.storage_failed"
                    } else {
                        "translation.live_incomplete"
                    }
                    .into(),
                    detail: storage_error.unwrap_or_else(|| {
                        "Live translation ended before this sentence was translated".into()
                    }),
                    target_language: &item.target_language,
                    preferred: true,
                    message_id,
                    source,
                });
        } else {
            let mut pending = self.pending_native.lock().expect("native translation lock");
            // Retain only the last completed sentence for a trailing target continuation.
            pending.retain(|id, item| {
                item.source != source || item.completed.is_none() || id == message_id
            });
            if let Some(item) = pending.get_mut(message_id) {
                item.completed = Some(translation.clone());
            }
            drop(pending);
            self.output.translation_completed_with_message(
                item.subtitle_id,
                translation,
                true,
                message_id,
                source,
            );
        }
        Ok(())
    }

    async fn publish_native_translation_group(
        &self,
        source: &'static str,
        result: crate::asr::LiveTranslationResult,
    ) -> Result<(), String> {
        let items = {
            let pending = self.pending_native.lock().expect("native translation lock");
            result
                .source_utterance_ids
                .iter()
                .map(|id| {
                    pending
                        .get(id)
                        .filter(|item| item.source == source)
                        .cloned()
                })
                .collect::<Option<Vec<_>>>()
        };
        let Some(items) = items else {
            return Ok(());
        };
        // OpenAI native results come only from Realtime Translation; preserve its text.
        let repair = if result.provider == crate::providers::OPENAI_PROVIDER {
            None
        } else {
            let config = self.config.read().expect("config lock");
            let language = self
                .language_session
                .read()
                .expect("language session lock")
                .resolve(&config);
            native_repair_target(
                &language.translation,
                &config.asr.api_profiles,
                source,
                &result.transcript.target_language,
            )
            .map(|target| {
                (
                    target,
                    language.translation.prompt,
                    config.asr.api_profiles.clone(),
                    config.vrcx.enabled && config.vrcx.include_in_llm_context,
                )
            })
        };
        if let Some((target, prompt, profiles, include_vrcx_context)) = repair {
            let subtitles = {
                let database = self
                    .database
                    .lock()
                    .map_err(|_| "Database lock is unavailable".to_string())?;
                items
                    .iter()
                    .map(|item| {
                        database
                            .subtitle(item.subtitle_id)
                            .map_err(|error| error.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
            };
            if let Some(subtitles) = subtitles {
                self.pending_native
                    .lock()
                    .expect("native translation lock")
                    .retain(|id, _| !result.source_utterance_ids.contains(id));
                for ((message_id, item), subtitle) in result
                    .source_utterance_ids
                    .iter()
                    .zip(&items)
                    .zip(subtitles)
                {
                    if let Err(detail) = self.translation.enqueue(
                        subtitle,
                        vec![target.clone()],
                        prompt.clone(),
                        profiles.clone(),
                        message_id.clone(),
                        include_vrcx_context,
                        true,
                    ) {
                        self.output
                            .translation_failed_with_message(TranslationFailure {
                                subtitle_id: item.subtitle_id,
                                code: "translation.queue_full".into(),
                                detail,
                                target_language: &target.target_language,
                                preferred: true,
                                message_id,
                                source,
                            });
                    }
                }
                return Ok(());
            }
        }
        let subtitle_ids = items
            .iter()
            .map(|item| item.subtitle_id)
            .collect::<Vec<_>>();
        let translation = SubtitleTranslation {
            source_group: Some(TranslationSourceGroup {
                subtitle_ids: subtitle_ids.clone(),
                text: result.transcript.text,
            }),
            text: result.transcript.translation.trim().to_owned(),
            source_language: result.transcript.language,
            target_language: result.transcript.target_language,
            provider: result.provider,
            model: Some(result.model),
            created_at: now_iso8601(),
        };
        let storage_error = {
            let database = Arc::clone(&self.database);
            let record = translation.clone();
            let catalog = self.conversation_catalog.clone();
            match tokio::task::spawn_blocking(move || {
                let database = database
                    .lock()
                    .map_err(|_| "Database lock is unavailable".to_string())?;
                let changed = database
                    .save_translation_group(&subtitle_ids, &record)
                    .map_err(|error| error.to_string())?;
                if changed {
                    publish_latest_catalog(&database, &catalog);
                }
                Ok::<_, String>(())
            })
            .await
            {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(error.to_string()),
            }
        };
        self.pending_native
            .lock()
            .expect("native translation lock")
            .retain(|id, _| !result.source_utterance_ids.contains(id));
        for (id, item) in result.source_utterance_ids.iter().zip(items) {
            if let Some(detail) = &storage_error {
                self.output
                    .translation_failed_with_message(TranslationFailure {
                        subtitle_id: item.subtitle_id,
                        code: "translation.storage_failed".into(),
                        detail: detail.clone(),
                        target_language: &item.target_language,
                        preferred: true,
                        message_id: id,
                        source,
                    });
            } else {
                self.output.translation_completed_with_message(
                    item.subtitle_id,
                    translation.clone(),
                    true,
                    id,
                    source,
                );
            }
        }
        Ok(())
    }

    async fn publish_text_with_translation(
        &self,
        text: String,
        language: Option<String>,
        source: &'static str,
        message_id: String,
        native: Option<SubtitleTranslation>,
        native_pending: bool,
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
        let native_record = native
            .clone()
            .filter(|translation| !translation.text.is_empty());
        let (saved, native_error) = match tokio::task::spawn_blocking(move || {
            let database = database
                .lock()
                .map_err(|_| "Database lock is unavailable".to_string())?;
            let mut saved = database
                .add_subtitle(&subtitle)
                .map_err(|error| error.to_string())?;
            let mut native_error = None;
            if let Some(translation) = native_record {
                match database.save_translation(saved.id.expect("saved subtitle id"), &translation)
                {
                    Ok(_) => saved.translations.push(translation),
                    Err(error) => native_error = Some(error.to_string()),
                }
            }
            publish_latest_catalog(&database, &conversation_catalog);
            Ok::<_, String>((saved, native_error))
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
        let (mut translation_targets, translation_prompt, api_profiles, include_vrcx_context) = {
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
        if let (Some(targets), Some(native)) = (&mut translation_targets, &native) {
            targets.retain(|target| target.target_language != native.target_language);
        }
        let mut output_targets = translation_targets
            .as_ref()
            .map(|targets| {
                targets
                    .iter()
                    .map(|target| target.target_language.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(native) = &native {
            if native_pending || !native.text.is_empty() {
                output_targets.insert(0, native.target_language.clone());
            }
        }
        self.output.subtitle_stored_with_message(
            saved.clone(),
            !output_targets.is_empty(),
            output_targets,
            &message_id,
        );
        if let Some(native) = &native {
            if let Some(detail) = native_error {
                self.output
                    .translation_failed_with_message(TranslationFailure {
                        subtitle_id: saved.id.unwrap(),
                        code: "translation.storage_failed".into(),
                        detail,
                        target_language: &native.target_language,
                        preferred: true,
                        message_id: &message_id,
                        source,
                    });
            } else if native_pending {
                self.pending_native
                    .lock()
                    .expect("native translation lock")
                    .insert(
                        message_id.clone(),
                        PendingNative {
                            subtitle_id: saved.id.unwrap(),
                            source: source.into(),
                            target_language: native.target_language.clone(),
                            completed: None,
                        },
                    );
                self.output.translation_started_with_message(
                    saved.id.unwrap(),
                    &native.target_language,
                    true,
                    &message_id,
                    source,
                );
            } else if !native.text.is_empty() {
                self.output.translation_completed_with_message(
                    saved.id.unwrap(),
                    native.clone(),
                    true,
                    &message_id,
                    source,
                );
            } else if !saved.language.as_deref().is_some_and(|language| {
                same_translation_language(language, &native.target_language)
            }) {
                self.output
                    .translation_failed_with_message(TranslationFailure {
                        subtitle_id: saved.id.unwrap(),
                        code: "translation.live_incomplete".into(),
                        detail: "Live translation ended before the translated sentence arrived"
                            .into(),
                        target_language: &native.target_language,
                        preferred: true,
                        message_id: &message_id,
                        source,
                    });
            }
        }
        if let Some(targets) = translation_targets {
            if targets.is_empty() {
                return Ok(());
            }
            let failed_targets = targets.clone();
            if let Err(detail) = self.translation.enqueue(
                saved.clone(),
                targets,
                translation_prompt,
                api_profiles,
                message_id.clone(),
                include_vrcx_context,
                native.is_none(),
            ) {
                if let Some(subtitle_id) = saved.id {
                    for (index, target) in failed_targets.iter().enumerate() {
                        self.output
                            .translation_failed_with_message(TranslationFailure {
                                subtitle_id,
                                code: "translation.queue_full".into(),
                                detail: detail.clone(),
                                target_language: &target.target_language,
                                preferred: native.is_none() && index == 0,
                                message_id: &message_id,
                                source,
                            });
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

fn native_repair_target(
    config: &TranslationConfig,
    profiles: &[crate::config::ApiProfile],
    source: &str,
    target_language: &str,
) -> Option<crate::config::TranslationTargetConfig> {
    automatic_translation_targets(config, source, None)?
        .into_iter()
        .find(|target| {
            target.target_language == target_language
                && target.profile_id.as_ref().is_some_and(|profile_id| {
                    profiles.iter().any(|profile| {
                        profile.id == *profile_id
                            && crate::providers::supports_translation_language(
                                profile,
                                target_language,
                            )
                    })
                })
        })
}

#[cfg(test)]
mod tests {
    use super::{automatic_translation_targets, native_repair_target};
    use crate::config::{ApiAuthMode, ApiProfile, TranslationConfig, TranslationTargetConfig};

    fn native_result() -> crate::asr::LiveTranslationResult {
        crate::asr::LiveTranslationResult {
            pending: false,
            source_utterance_ids: vec!["native-1".into()],
            provider: crate::providers::GEMINI_PROVIDER.into(),
            transcript: crate::models::LiveTranslation {
                utterance_id: "native-1".into(),
                text: "Hello.".into(),
                language: Some("en".into()),
                translation: "你好。".into(),
                target_language: "zh-Hans".into(),
            },
            model: "gemini-3.5-live-translate-preview".into(),
        }
    }

    #[test]
    fn native_repair_requires_the_configured_text_translation_profile() {
        let mut target = TranslationTargetConfig::new("zh-Hans");
        target.profile_id = Some("local".into());
        let config = TranslationConfig {
            mode: "automatic".into(),
            microphone_targets: vec![target.clone()],
            ..TranslationConfig::default()
        };
        let profile = ApiProfile {
            id: "local".into(),
            provider: crate::providers::OLLAMA_PROVIDER.into(),
            enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
            auth_mode: ApiAuthMode::None,
            is_local: true,
            ..ApiProfile::default()
        };

        assert_eq!(
            native_repair_target(&config, &[profile], "microphone", "zh-Hans"),
            Some(target)
        );
        assert!(native_repair_target(&config, &[], "microphone", "zh-Hans").is_none());
        assert!(native_repair_target(&config, &[], "microphone", "ja").is_none());
    }

    #[tokio::test]
    async fn native_sentences_stream_and_complete_on_their_original_rows_without_llm_jobs() {
        let dependencies =
            super::super::tests::test_dependencies(crate::domain_events::DomainEventHub::new());
        let mut events = dependencies.output.subscribe_translations();
        {
            let mut config = dependencies.config.write().unwrap();
            config.translation.mode = "automatic".into();
            config.translation.microphone_targets = vec![TranslationTargetConfig::new("zh-Hans")];
        }
        for (id, text) in [("first", "Hello."), ("second", "How are you?")] {
            let mut result = native_result();
            result.pending = true;
            result.transcript.utterance_id = id.into();
            result.transcript.text = text.into();
            result.transcript.translation.clear();
            dependencies
                .publish_native_translation("microphone", result)
                .await
                .unwrap();
            assert!(matches!(
                events.try_recv().unwrap(),
                crate::subtitle_output::TranslationEvent::TranslationStarted { .. }
            ));
        }
        let before = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        let mut partial = native_result();
        partial.pending = true;
        partial.transcript.utterance_id = "first".into();
        partial.transcript.translation = "你".into();
        dependencies
            .publish_native_translation_update("microphone", partial.clone())
            .await
            .unwrap();
        assert!(
            matches!(events.try_recv().unwrap(), crate::subtitle_output::TranslationEvent::TranslationPartial { text, .. } if text == "你")
        );
        partial.transcript.translation.clear();
        dependencies
            .publish_native_translation_update("microphone", partial)
            .await
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap(),
            crate::subtitle_output::TranslationEvent::TranslationStarted { .. }
        ));
        assert!(dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap()
            .iter()
            .all(|subtitle| subtitle.translations.is_empty()));
        for (id, text) in [("first", "你好。"), ("second", "最近怎么样？")] {
            let mut result = native_result();
            result.transcript.utterance_id = id.into();
            result.transcript.translation = text.into();
            dependencies
                .publish_native_translation_update("microphone", result.clone())
                .await
                .unwrap();
            assert!(
                matches!(events.try_recv().unwrap(), crate::subtitle_output::TranslationEvent::TranslationCompleted { translation, .. } if translation.text == text && translation.source_group.is_none())
            );
            dependencies
                .publish_native_translation_update("microphone", result)
                .await
                .unwrap();
            assert!(events.try_recv().is_err());
        }
        let after = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        assert_eq!(
            after.iter().map(|subtitle| subtitle.id).collect::<Vec<_>>(),
            before
                .iter()
                .map(|subtitle| subtitle.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            after
                .iter()
                .find(|subtitle| subtitle.text == "Hello.")
                .unwrap()
                .translations[0]
                .text,
            "你好。"
        );
        assert_eq!(
            after
                .iter()
                .find(|subtitle| subtitle.text == "How are you?")
                .unwrap()
                .translations[0]
                .text,
            "最近怎么样？"
        );
        assert_eq!(dependencies.pending_native.lock().unwrap().len(), 1);
        let mut continuation = native_result();
        continuation.transcript.utterance_id = "second".into();
        continuation.transcript.translation = "最近怎么样？ 还好吗？".into();
        dependencies
            .publish_native_translation_update("microphone", continuation)
            .await
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap(),
            crate::subtitle_output::TranslationEvent::TranslationCompleted { .. }
        ));
        let continued = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        assert_eq!(continued.len(), 2);
        assert_eq!(
            continued
                .iter()
                .find(|subtitle| subtitle.text == "How are you?")
                .unwrap()
                .translations[0]
                .text,
            "最近怎么样？ 还好吗？"
        );
        dependencies.reset_recognition("microphone");
        assert!(dependencies.pending_native.lock().unwrap().is_empty());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), events.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn native_group_is_saved_and_completed_for_every_source_row() {
        let dependencies =
            super::super::tests::test_dependencies(crate::domain_events::DomainEventHub::new());
        let mut events = dependencies.output.subscribe_translations();
        for (id, text) in [("first", "First."), ("second", "Second.")] {
            let mut result = native_result();
            result.pending = true;
            result.source_utterance_ids = vec![id.into()];
            result.transcript.utterance_id = id.into();
            result.transcript.text = text.into();
            result.transcript.translation.clear();
            dependencies
                .publish_native_translation("microphone", result)
                .await
                .unwrap();
            events.try_recv().unwrap();
        }

        let mut result = native_result();
        result.source_utterance_ids = vec!["first".into(), "second".into()];
        result.transcript.text = "First. Second.".into();
        result.transcript.translation = "第一句。 第二句。".into();
        dependencies
            .publish_native_translation_update("microphone", result)
            .await
            .unwrap();

        let history = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        let expected_ids = ["First.", "Second."]
            .iter()
            .map(|text| {
                history
                    .iter()
                    .find(|row| row.text == *text)
                    .unwrap()
                    .id
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for expected_id in &expected_ids {
            let event = events.try_recv().unwrap();
            let crate::subtitle_output::TranslationEvent::TranslationCompleted {
                subtitle_id,
                translation,
                ..
            } = event
            else {
                panic!()
            };
            assert_eq!(subtitle_id, *expected_id);
            assert_eq!(translation.text, "第一句。 第二句。");
            assert_eq!(translation.source_group.unwrap().subtitle_ids, expected_ids);
        }
        assert!(dependencies.pending_native.lock().unwrap().is_empty());
        assert!(history.iter().all(|row| {
            row.translations.len() == 1
                && row.translations[0]
                    .source_group
                    .as_ref()
                    .is_some_and(|group| group.subtitle_ids == expected_ids)
        }));
    }

    #[tokio::test]
    async fn native_group_uses_individual_translation_jobs_when_a_profile_is_available() {
        let dependencies =
            super::super::tests::test_dependencies(crate::domain_events::DomainEventHub::new());
        let mut events = dependencies.output.subscribe_translations();
        {
            let mut config = dependencies.config.write().unwrap();
            let mut target = TranslationTargetConfig::new("zh-Hans");
            target.profile_id = Some("repair".into());
            config.translation.mode = "automatic".into();
            config.translation.microphone_targets = vec![target];
            config.asr.api_profiles.push(ApiProfile {
                id: "repair".into(),
                provider: crate::providers::OLLAMA_PROVIDER.into(),
                base_url: Some("http://127.0.0.1:9/v1".into()),
                enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
                auth_mode: ApiAuthMode::None,
                is_local: true,
                timeout_ms: 100,
                ..ApiProfile::default()
            });
        }
        for (id, text) in [("first", "First."), ("second", "Second.")] {
            let mut result = native_result();
            result.pending = true;
            result.source_utterance_ids = vec![id.into()];
            result.transcript.utterance_id = id.into();
            result.transcript.text = text.into();
            result.transcript.translation.clear();
            dependencies
                .publish_native_translation("microphone", result)
                .await
                .unwrap();
            events.try_recv().unwrap();
        }

        let mut result = native_result();
        result.source_utterance_ids = vec!["first".into(), "second".into()];
        result.transcript.text = "First. Second.".into();
        result.transcript.translation = "第一句和第二句。".into();
        dependencies
            .publish_native_translation_update("microphone", result)
            .await
            .unwrap();

        let mut started = 0;
        while started < 2 {
            let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
                .await
                .unwrap()
                .unwrap();
            if matches!(
                event,
                crate::subtitle_output::TranslationEvent::TranslationStarted { .. }
            ) {
                started += 1;
            }
        }
        assert!(dependencies.pending_native.lock().unwrap().is_empty());
        assert!(dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap()
            .iter()
            .all(|subtitle| subtitle.translations.is_empty()));
    }

    #[tokio::test]
    async fn native_group_preserves_translation_when_a_text_profile_is_available() {
        let dependencies =
            super::super::tests::test_dependencies(crate::domain_events::DomainEventHub::new());
        let mut events = dependencies.output.subscribe_translations();
        {
            let mut config = dependencies.config.write().unwrap();
            let mut target = TranslationTargetConfig::new("zh-Hans");
            target.profile_id = Some("repair".into());
            config.translation.mode = "automatic".into();
            config.translation.microphone_targets = vec![target];
            config.asr.api_profiles.push(ApiProfile {
                id: "repair".into(),
                provider: crate::providers::OLLAMA_PROVIDER.into(),
                base_url: Some("http://127.0.0.1:9/v1".into()),
                enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
                auth_mode: ApiAuthMode::None,
                is_local: true,
                timeout_ms: 100,
                ..ApiProfile::default()
            });
        }
        for (id, text) in [("first", "First."), ("second", "Second.")] {
            let mut result = native_result();
            result.provider = crate::providers::OPENAI_PROVIDER.into();
            result.pending = true;
            result.source_utterance_ids = vec![id.into()];
            result.transcript.utterance_id = id.into();
            result.transcript.text = text.into();
            result.transcript.translation.clear();
            dependencies
                .publish_native_translation("microphone", result)
                .await
                .unwrap();
            events.try_recv().unwrap();
        }

        let mut result = native_result();
        result.provider = crate::providers::OPENAI_PROVIDER.into();
        result.source_utterance_ids = vec!["first".into(), "second".into()];
        result.transcript.text = "First. Second.".into();
        result.transcript.translation = "第一句和第二句。".into();
        dependencies
            .publish_native_translation_update("microphone", result)
            .await
            .unwrap();

        for _ in 0..2 {
            assert!(matches!(
                events.try_recv().unwrap(),
                crate::subtitle_output::TranslationEvent::TranslationCompleted { .. }
            ));
        }
        assert!(events.try_recv().is_err());
        assert!(dependencies.pending_native.lock().unwrap().is_empty());
        assert!(dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap()
            .iter()
            .all(
                |row| row.translations.len() == 1 && row.translations[0].text == "第一句和第二句。"
            ));
    }

    #[tokio::test]
    async fn reset_fails_only_its_pending_sources_and_rejects_late_updates() {
        let dependencies =
            super::super::tests::test_dependencies(crate::domain_events::DomainEventHub::new());
        let mut events = dependencies.output.subscribe_translations();
        for source in ["microphone", "speaker"] {
            let mut result = native_result();
            result.pending = true;
            result.transcript.translation.clear();
            result.transcript.utterance_id = source.into();
            dependencies
                .publish_native_translation(source, result)
                .await
                .unwrap();
            events.try_recv().unwrap();
        }
        dependencies.reset_recognition("microphone");
        assert!(matches!(
            events.try_recv().unwrap(),
            crate::subtitle_output::TranslationEvent::TranslationFailed { .. }
        ));
        assert_eq!(dependencies.pending_native.lock().unwrap().len(), 1);
        dependencies
            .publish_native_translation_update(
                "microphone",
                crate::asr::LiveTranslationResult {
                    transcript: crate::models::LiveTranslation {
                        utterance_id: "microphone".into(),
                        ..native_result().transcript
                    },
                    ..native_result()
                },
            )
            .await
            .unwrap();
        assert!(events.try_recv().is_err());
        assert!(dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap()
            .iter()
            .all(|subtitle| subtitle.translations.is_empty()));
    }

    #[tokio::test]
    async fn native_translation_is_stored_and_extra_targets_are_not_preferred() {
        for (provider, model) in [
            ("gemini", "gemini-3.5-live-translate-preview"),
            ("openai", "gpt-realtime-translate"),
        ] {
            let events = crate::domain_events::DomainEventHub::new();
            let dependencies = super::super::tests::test_dependencies(events);
            let mut presentation = dependencies.output.subscribe_presentation_events();
            let mut translation_events = dependencies.output.subscribe_translations();
            {
                let mut config = dependencies.config.write().unwrap();
                config.translation.mode = "automatic".into();
                config.translation.speaker_targets = vec![
                    TranslationTargetConfig::new("zh-Hans"),
                    TranslationTargetConfig::new("ja"),
                ];
            }
            let mut result = native_result();
            result.transcript.text = "Hello. How are you?".into();
            result.transcript.translation = "你好，最近怎么样？很高兴见到你。".into();
            result.provider = provider.into();
            result.model = model.into();
            dependencies
                .publish_native_translation("speaker", result)
                .await
                .unwrap();
            let history = dependencies
                .database
                .lock()
                .unwrap()
                .subtitle_history(10)
                .unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].text, "Hello. How are you?");
            assert_eq!(history[0].translations.len(), 1);
            assert_eq!(
                history[0].translations[0].text,
                "你好，最近怎么样？很高兴见到你。"
            );
            assert_eq!(history[0].translations[0].provider, provider);
            assert_eq!(history[0].translations[0].model.as_deref(), Some(model));
            assert!(matches!(presentation.try_recv().unwrap(),
            crate::subtitle_output::PresentationEvent::Final { subtitle, .. }
                if subtitle.translations.len() == 1));
            assert!(matches!(
                translation_events.recv().await.unwrap(),
                crate::subtitle_output::TranslationEvent::TranslationCompleted {
                    preferred: true,
                    ..
                }
            ));
            let event =
                tokio::time::timeout(std::time::Duration::from_secs(2), translation_events.recv())
                    .await
                    .unwrap()
                    .unwrap();
            assert!(matches!(event,
            crate::subtitle_output::TranslationEvent::TranslationStarted { target_language, preferred: false, .. }
                if target_language == "ja"));
        }
    }

    #[tokio::test]
    async fn failed_native_translation_storage_preserves_source_without_success_event() {
        let events = crate::domain_events::DomainEventHub::new();
        let dependencies = super::super::tests::test_dependencies(events);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("storage-failure.db");
        *dependencies.database.lock().unwrap() = crate::db::Database::open(&path).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TRIGGER fail_translation BEFORE INSERT ON subtitle_translations BEGIN SELECT RAISE(FAIL, 'test storage failure'); END;").unwrap();
        let mut translations = dependencies.output.subscribe_translations();
        dependencies
            .publish_native_translation("speaker", native_result())
            .await
            .unwrap();
        let history = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        assert_eq!(history[0].text, "Hello.");
        assert!(history[0].translations.is_empty());
        assert!(matches!(translations.try_recv().unwrap(),
            crate::subtitle_output::TranslationEvent::TranslationFailed { code, .. }
                if code == "translation.storage_failed"));
        assert!(translations.try_recv().is_err());
    }

    #[tokio::test]
    async fn native_only_does_not_enqueue_llm_and_missing_translation_keeps_source() {
        let events = crate::domain_events::DomainEventHub::new();
        let dependencies = super::super::tests::test_dependencies(events);
        let mut translations = dependencies.output.subscribe_translations();
        {
            let mut config = dependencies.config.write().unwrap();
            config.translation.mode = "automatic".into();
            config.translation.microphone_targets = vec![TranslationTargetConfig::new("zh-Hans")];
        }
        dependencies
            .publish_native_translation("microphone", native_result())
            .await
            .unwrap();
        assert!(matches!(
            translations.try_recv().unwrap(),
            crate::subtitle_output::TranslationEvent::TranslationCompleted { .. }
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), translations.recv())
                .await
                .is_err()
        );
        let mut missing = native_result();
        missing.transcript.utterance_id = "native-2".into();
        missing.transcript.translation.clear();
        dependencies
            .publish_native_translation("microphone", missing)
            .await
            .unwrap();
        let history = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].source, "microphone");
        assert_eq!(history[0].text, "Hello.");
        assert!(history[0].translations.is_empty());
        assert!(matches!(translations.try_recv().unwrap(),
            crate::subtitle_output::TranslationEvent::TranslationFailed { code, .. }
                if code == "translation.live_incomplete"));
    }

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
            speaker_targets: vec![TranslationTargetConfig::new("zh-Hans")],
            microphone_targets: vec![TranslationTargetConfig::new("ja")],
            ..TranslationConfig::default()
        };

        assert!(automatic_translation_targets(&config, "speaker", Some("zh")).is_none());
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
