use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock, Weak};

use tokio::sync::broadcast;

use crate::asr::{ModelManager, QwenRuntime};
use crate::config::{AppConfig, TranslationConfig};
use crate::db::conversations::{CatalogRefresh, ConversationCatalog};
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

struct RecognizedText {
    text: String,
    language: Option<String>,
    speaker: Option<crate::models::SpeakerIdentity>,
}

type PlayerIdentity = Arc<RwLock<crate::models::SpeakerIdentity>>;
type PlayerIdentities = Arc<Mutex<HashMap<String, Weak<RwLock<crate::models::SpeakerIdentity>>>>>;

#[derive(Clone)]
pub(crate) struct PipelineDependencies {
    database: Arc<Mutex<Database>>,
    live: broadcast::Sender<LiveTranscription>,
    catalog: CatalogRefresh,
    translation: TranslationDispatcher,
    config: Arc<std::sync::RwLock<AppConfig>>,
    language_session: Arc<std::sync::RwLock<crate::language_session::ActiveLanguageSession>>,
    output: SubtitleLifecyclePublisher,
    pending_native: Arc<Mutex<HashMap<String, PendingNative>>>,
    managed_qwen: Option<(Arc<QwenRuntime>, Arc<ModelManager>)>,
    player: Option<(PlayerIdentity, String)>,
    player_identities: PlayerIdentities,
    player_previews: Arc<Mutex<HashSet<String>>>,
}

impl PipelineDependencies {
    pub(super) fn player_settings(&self) -> Arc<RwLock<AppConfig>> {
        Arc::clone(&self.config)
    }

    pub(super) fn player_selected(&self, speaker: &crate::models::SpeakerIdentity) -> bool {
        self.config
            .read()
            .expect("config lock")
            .audio
            .output
            .player_audio
            .selected_tracks
            .as_ref()
            .is_none_or(|ids| speaker.user_id.as_ref().is_some_and(|id| ids.contains(id)))
    }

    pub(super) fn current_player_selected(&self) -> bool {
        self.player_identity()
            .is_none_or(|player| self.player_selected(&player))
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        database: Arc<Mutex<Database>>,
        live: broadcast::Sender<LiveTranscription>,
        conversation_catalog: broadcast::Sender<ConversationCatalog>,
        translation: TranslationDispatcher,
        config: Arc<std::sync::RwLock<AppConfig>>,
        language_session: Arc<std::sync::RwLock<crate::language_session::ActiveLanguageSession>>,
        output: SubtitleLifecyclePublisher,
    ) -> Self {
        Self {
            catalog: CatalogRefresh::new(Arc::clone(&database), conversation_catalog),
            database,
            live,
            translation,
            config,
            language_session,
            output,
            pending_native: Arc::default(),
            managed_qwen: None,
            player: None,
            player_identities: Arc::default(),
            player_previews: Arc::default(),
        }
    }

    pub(crate) fn with_managed_qwen(
        mut self,
        runtime: Arc<QwenRuntime>,
        manager: Arc<ModelManager>,
    ) -> Self {
        self.managed_qwen = Some((runtime, manager));
        self
    }

    pub(crate) fn managed_qwen(&self) -> Option<(Arc<QwenRuntime>, Arc<ModelManager>)> {
        self.managed_qwen.clone()
    }

    pub(crate) fn with_player(mut self, speaker: crate::models::SpeakerIdentity) -> Self {
        let shared = {
            let mut identities = self.player_identities.lock().expect("player identities");
            identities.retain(|_, value| value.strong_count() > 0);
            let shared = identities
                .get(&speaker.id)
                .and_then(Weak::upgrade)
                .unwrap_or_else(|| Arc::new(RwLock::new(speaker.clone())));
            if speaker.display_name.is_some() {
                shared
                    .write()
                    .expect("player identity")
                    .display_name
                    .clone_from(&speaker.display_name);
            }
            if speaker.user_id.is_some() {
                shared
                    .write()
                    .expect("player identity")
                    .user_id
                    .clone_from(&speaker.user_id);
            }
            identities.insert(speaker.id.clone(), Arc::downgrade(&shared));
            shared
        };
        self.player = Some((shared, format!("player-{}", uuid::Uuid::new_v4())));
        self.pending_native = Arc::default();
        self.player_previews = Arc::default();
        self
    }

    fn player_identity(&self) -> Option<crate::models::SpeakerIdentity> {
        self.player
            .as_ref()
            .map(|(speaker, _)| speaker.read().expect("player identity").clone())
    }

    pub(super) fn update_player(&self, speaker: &crate::models::SpeakerIdentity) -> bool {
        let Some((current, _)) = &self.player else {
            return false;
        };
        let mut current = current.write().expect("player identity");
        if current.id != speaker.id
            || (current.display_name == speaker.display_name && current.user_id == speaker.user_id)
        {
            return false;
        }
        current.display_name.clone_from(&speaker.display_name);
        current.user_id.clone_from(&speaker.user_id);
        true
    }

    pub(super) async fn publish_speaker(&self, speaker: crate::models::SpeakerIdentity) {
        // Retiring ASR tasks can still be saving their last sentence.
        // They share this identity even after removal from the active task map.
        if let Some(shared) = self
            .player_identities
            .lock()
            .expect("player identities")
            .get(&speaker.id)
            .and_then(Weak::upgrade)
        {
            let mut current = shared.write().expect("player identity");
            current.display_name.clone_from(&speaker.display_name);
            current.user_id.clone_from(&speaker.user_id);
        }
        let database = Arc::clone(&self.database);
        let update = speaker.clone();
        match tokio::task::spawn_blocking(move || {
            database
                .lock()
                .expect("database")
                .update_speaker_identity(&update)
        })
        .await
        {
            Ok(Ok(())) => {}
            _ => tracing::warn!("Could not update stored player display names"),
        }
        self.output.speaker_updated(&speaker);
    }

    pub(super) fn scope_id(&self, id: &str) -> String {
        self.player
            .as_ref()
            .map_or_else(|| id.to_owned(), |(_, prefix)| format!("{prefix}:{id}"))
    }

    pub(super) fn scope_event(
        &self,
        mut event: crate::asr::CloudEvent,
        source: &str,
    ) -> crate::asr::CloudEvent {
        if source == "microphone" {
            let speakers = self.output.microphone_speakers();
            match &mut event {
                crate::asr::CloudEvent::SpeakerSession { generation } => {
                    speakers.begin_session(generation.clone());
                    self.output.asr_reset(source);
                }
                crate::asr::CloudEvent::LiveTranslation {
                    snapshot,
                    completed,
                    translations,
                    ..
                } => {
                    for transcript in completed
                        .iter()
                        .chain(translations.iter())
                        .map(|item| &item.transcript)
                        .chain(std::iter::once(snapshot.as_ref()))
                    {
                        speakers.discover(&transcript.utterance_id, transcript.speaker.as_ref());
                        if let Some(id) = &transcript.source_utterance_id {
                            speakers.discover(id, transcript.speaker.as_ref());
                        }
                    }
                    // Unresolved originals can be emitted again when native speaker metadata arrives.
                    completed
                        .retain(|result| speakers.allows(source, &result.transcript.utterance_id));
                    translations
                        .retain(|result| speakers.allows(source, &result.transcript.utterance_id));
                }
                crate::asr::CloudEvent::Failed {
                    reset_session: true,
                    ..
                } => speakers.begin_session(String::new()),
                crate::asr::CloudEvent::Partial { utterance_id, .. }
                | crate::asr::CloudEvent::Final { utterance_id, .. } => {
                    speakers.discover(utterance_id, None)
                }
                _ => {}
            }
            self.pending_native
                .lock()
                .expect("native translation lock")
                .retain(|id, item| {
                    item.source != "microphone" || speakers.allows("microphone", id)
                });
        }
        let Some(speaker) = self.player_identity() else {
            return event;
        };
        let transcript = |value: &mut crate::models::LiveTranslation| {
            value.utterance_id = self.scope_id(&value.utterance_id);
            value.source_utterance_id = value
                .source_utterance_id
                .as_ref()
                .map(|id| self.scope_id(id));
            value.speaker = Some(speaker.clone());
        };
        match &mut event {
            crate::asr::CloudEvent::SpeakerSession { .. } => {}
            crate::asr::CloudEvent::Partial { utterance_id, .. }
            | crate::asr::CloudEvent::Final { utterance_id, .. } => {
                *utterance_id = self.scope_id(utterance_id)
            }
            crate::asr::CloudEvent::Failed { utterance_id, .. } => {
                *utterance_id = utterance_id.as_ref().map(|id| self.scope_id(id))
            }
            crate::asr::CloudEvent::LiveTranslation {
                snapshot,
                completed,
                translations,
                finished_preview_ids,
                ..
            } => {
                transcript(snapshot);
                for result in completed.iter_mut().chain(translations) {
                    transcript(&mut result.transcript);
                    result.source_utterance_ids = result
                        .source_utterance_ids
                        .iter()
                        .map(|id| self.scope_id(id))
                        .collect();
                }
                *finished_preview_ids = finished_preview_ids
                    .iter()
                    .map(|id| self.scope_id(id))
                    .collect();
            }
        }
        event
    }

    pub(crate) fn publish_live_translation(
        &self,
        source: &str,
        snapshot: crate::models::LiveTranslation,
    ) {
        if !self.current_player_selected() {
            return;
        }
        if self.player.is_some() {
            self.player_previews
                .lock()
                .expect("player previews")
                .insert(snapshot.utterance_id.clone());
        }
        self.output.live_translation(source, snapshot);
    }

    pub(crate) fn publish_live(&self, event: LiveTranscription) {
        if !self.current_player_selected() {
            return;
        }
        match &event {
            LiveTranscription::Partial {
                utterance_id,
                source,
                text,
                language,
            } => {
                if !self.output.allows(source, utterance_id) {
                    return;
                }
                if let Some(speaker) = self.player_identity() {
                    self.player_previews
                        .lock()
                        .expect("player previews")
                        .insert(utterance_id.clone());
                    self.output.asr_player_partial(
                        utterance_id,
                        source,
                        text,
                        language.as_deref(),
                        &speaker,
                    );
                } else {
                    self.output
                        .asr_partial(utterance_id, source, text, language.as_deref());
                }
            }
            LiveTranscription::Failed {
                utterance_id,
                source,
                code,
                detail,
            } => {
                if self.player.is_some() {
                    if let Some(id) = utterance_id {
                        self.player_previews
                            .lock()
                            .expect("player previews")
                            .remove(id);
                    } else {
                        self.reset_recognition(source);
                    }
                }
                let scoped = self
                    .player
                    .as_ref()
                    .map(|(_, prefix)| format!("{prefix}:session"));
                self.output.asr_failed(
                    utterance_id.as_deref().or(scoped.as_deref()),
                    source,
                    code,
                    detail,
                );
            }
            LiveTranscription::AudioLevel { .. } => {}
        }
        if let LiveTranscription::Failed { source, .. } = &event {
            self.fail_pending_native(source);
        }
        let _ = self.live.send(event);
    }

    pub(crate) fn cancel_recognition(&self, utterance_id: &str, source: &str, reason: &str) {
        self.player_previews
            .lock()
            .expect("player previews")
            .remove(utterance_id);
        self.output.asr_cancelled(utterance_id, source, reason);
    }

    pub(crate) fn reset_recognition(&self, source: &str) {
        if source == "microphone" {
            self.output.microphone_speakers().stop();
        }
        self.fail_pending_native(source);
        if self.player.is_none() {
            self.output.asr_reset(source);
        } else {
            for id in self
                .player_previews
                .lock()
                .expect("player previews")
                .drain()
            {
                self.output
                    .asr_cancelled(&id, source, "player_session_ended");
            }
        }
    }

    pub(crate) async fn publish_text(
        &self,
        text: String,
        language: Option<String>,
        source: &'static str,
        message_id: String,
    ) -> Result<(), String> {
        self.publish_text_with_translation(
            RecognizedText {
                text,
                language,
                speaker: self.player_identity(),
            },
            source,
            message_id,
            None,
            false,
        )
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
            RecognizedText {
                text: transcript.text,
                language: transcript.language,
                speaker: transcript.speaker,
            },
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
        if !self.current_player_selected() {
            return Ok(());
        }
        if !self.output.allows(source, &result.transcript.utterance_id) {
            return Ok(());
        }
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
            let catalog = self.catalog.clone();
            let filter = self.output.microphone_speakers().clone();
            let message_id = message_id.clone();
            match tokio::task::spawn_blocking(move || {
                filter
                    .with_allowed(source, &message_id, || {
                        let database = database
                            .lock()
                            .map_err(|_| "Database lock is unavailable".to_string())?;
                        let changed = database
                            .save_translation(item.subtitle_id, &record)
                            .map_err(|error| error.to_string())?;
                        if changed {
                            catalog.request();
                        }
                        Ok::<_, String>(())
                    })
                    .unwrap_or(Ok(()))
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
        if result
            .source_utterance_ids
            .iter()
            .any(|id| !self.output.allows(source, id))
        {
            return Ok(());
        }
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
            let catalog = self.catalog.clone();
            match tokio::task::spawn_blocking(move || {
                let database = database
                    .lock()
                    .map_err(|_| "Database lock is unavailable".to_string())?;
                let changed = database
                    .save_translation_group(&subtitle_ids, &record)
                    .map_err(|error| error.to_string())?;
                if changed {
                    catalog.request();
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
        transcript: RecognizedText,
        source: &'static str,
        message_id: String,
        native: Option<SubtitleTranslation>,
        native_pending: bool,
    ) -> Result<(), String> {
        if !self.output.allows(source, &message_id) {
            return Ok(());
        }
        if !self.current_player_selected() {
            self.output
                .asr_cancelled(&message_id, source, "track_unselected");
            return Ok(());
        }
        let RecognizedText {
            text,
            language,
            speaker,
        } = transcript;
        self.player_previews
            .lock()
            .expect("player previews")
            .remove(&message_id);
        let text = text.trim().to_string();
        if text.is_empty() {
            self.output.asr_cancelled(&message_id, source, "empty");
            return Ok(());
        }
        let mut subtitle = Subtitle {
            speaker,
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
        let player = self.player.as_ref().map(|(speaker, _)| Arc::clone(speaker));
        let catalog = self.catalog.clone();
        let native_record = native
            .clone()
            .filter(|translation| !translation.text.is_empty());
        let filter = self.output.microphone_speakers().clone();
        let storage_id = message_id.clone();
        let (saved, native_error) = match tokio::task::spawn_blocking(move || {
            filter
                .with_allowed(source, &storage_id, || {
                    let database = database
                        .lock()
                        .map_err(|_| "Database lock is unavailable".to_string())?;
                    // Read under the database lock so a late metadata update cannot
                    // backfill history and then be overwritten by an older ASR snapshot.
                    if let Some(player) = player {
                        subtitle.speaker = Some(player.read().expect("player identity").clone());
                    }
                    let mut saved = database
                        .add_subtitle(&subtitle)
                        .map_err(|error| error.to_string())?;
                    let mut native_error = None;
                    if let Some(translation) = native_record {
                        match database
                            .save_translation(saved.id.expect("saved subtitle id"), &translation)
                        {
                            Ok(_) => saved.translations.push(translation),
                            Err(error) => native_error = Some(error.to_string()),
                        }
                    }
                    catalog.request();
                    Ok::<_, String>((saved, native_error))
                })
                .transpose()
        })
        .await
        {
            Ok(Ok(Some(saved))) => saved,
            Ok(Ok(None)) => return Ok(()),
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
        if !self.output.allows(source, &message_id) {
            return Ok(());
        }
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
                    || (native.provider == crate::providers::GEMINI_PROVIDER
                        && crate::providers::same_live_translation_language(
                            language,
                            &native.target_language,
                        ))
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

    #[tokio::test]
    async fn deselected_track_discards_late_previews_and_results_without_affecting_selected_track()
    {
        use crate::models::{LiveTranscription, SpeakerIdentity};
        let hub = crate::domain_events::DomainEventHub::new();
        let mut events = hub.subscribe();
        let base = super::super::tests::test_dependencies(hub);
        let first = base.clone().with_player(SpeakerIdentity {
            id: "track-a".into(),
            index: 0,
            display_name: None,
            user_id: Some("usr_00000000-0000-4000-8000-000000000001".into()),
        });
        let second = base.clone().with_player(SpeakerIdentity {
            id: "track-b".into(),
            index: 1,
            display_name: None,
            user_id: Some("usr_00000000-0000-4000-8000-000000000002".into()),
        });
        base.config
            .write()
            .unwrap()
            .audio
            .output
            .player_audio
            .selected_tracks = Some(vec!["usr_00000000-0000-4000-8000-000000000001".into()]);
        second.publish_live(LiveTranscription::Partial {
            utterance_id: "late-b".into(),
            source: "speaker".into(),
            text: "must not display".into(),
            language: None,
        });
        second.publish_live_translation("speaker", native_result().transcript);
        assert!(events.try_recv().is_err());
        second
            .publish_text("must not save".into(), None, "speaker", "late-b".into())
            .await
            .unwrap();
        first
            .publish_text("selected track".into(), None, "speaker", "a".into())
            .await
            .unwrap();
        let history = base.database.lock().unwrap().subtitle_history(20).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].speaker.as_ref().unwrap().id, "track-a");
        // Returning to the default accepts new tracks again.
        base.config
            .write()
            .unwrap()
            .audio
            .output
            .player_audio
            .selected_tracks = None;
        second
            .publish_text("enabled again".into(), None, "speaker", "new-b".into())
            .await
            .unwrap();
        assert_eq!(
            base.database
                .lock()
                .unwrap()
                .subtitle_history(20)
                .unwrap()
                .len(),
            2
        );
    }

    fn native_result() -> crate::asr::LiveTranslationResult {
        crate::asr::LiveTranslationResult {
            pending: false,
            source_utterance_ids: vec!["native-1".into()],
            provider: crate::providers::GEMINI_PROVIDER.into(),
            transcript: crate::models::LiveTranslation {
                source_utterance_id: None,
                completed_original: None,
                conversation_preview: None,
                speaker: None,
                utterance_id: "native-1".into(),
                text: "Hello.".into(),
                language: Some("en".into()),
                translation: "你好。".into(),
                target_language: "zh-Hans".into(),
            },
            model: "gemini-3.5-live-translate-preview".into(),
        }
    }

    #[tokio::test]
    async fn microphone_speakers_filter_storage_previews_and_late_translations() {
        use crate::microphone_speakers::{SpeakerDecision, SpeakerSelection};
        let hub = crate::domain_events::DomainEventHub::new();
        let mut events = hub.subscribe();
        let dependencies = super::super::tests::test_dependencies(hub);
        let filter = dependencies.output.microphone_speakers();
        filter.start(true);
        filter.begin_session("session".into());
        let identity = |id: &str, index| crate::models::SpeakerIdentity {
            id: id.into(),
            index,
            display_name: None,
            user_id: None,
        };
        let alice = identity("alice", 0);
        let bob = identity("bob", 1);
        filter.discover("setup-a", Some(&alice));
        filter.discover("setup-b", Some(&bob));
        let snapshot = filter.snapshot();
        filter
            .apply(
                "session",
                snapshot.revision,
                true,
                &[
                    SpeakerSelection {
                        id: "alice".into(),
                        decision: SpeakerDecision::Allow,
                    },
                    SpeakerSelection {
                        id: "bob".into(),
                        decision: SpeakerDecision::Block,
                    },
                ],
            )
            .unwrap();
        for (id, speaker) in [
            ("allowed", Some(alice)),
            ("blocked", Some(bob)),
            ("unknown", None),
        ] {
            let mut result = native_result();
            result.pending = true;
            result.transcript.utterance_id = format!("qwen-source-session-{id}");
            result.transcript.translation.clear();
            result.transcript.speaker = speaker;
            dependencies.scope_event(
                crate::asr::CloudEvent::LiveTranslation {
                    service: crate::providers::SERVICE_QWEN_LIVE_TRANSLATE.into(),
                    snapshot: Box::new(result.transcript.clone()),
                    completed: vec![result.clone()],
                    translations: vec![],
                    finished_preview_ids: vec![],
                },
                "microphone",
            );
            dependencies.publish_live_translation("microphone", result.transcript.clone());
            dependencies
                .publish_native_translation("microphone", result)
                .await
                .unwrap();
        }
        let rows = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].speaker.as_ref().unwrap().id, "alice");
        let snapshot = filter.snapshot();
        filter
            .apply(
                "session",
                snapshot.revision,
                true,
                &[SpeakerSelection {
                    id: "alice".into(),
                    decision: SpeakerDecision::Block,
                }],
            )
            .unwrap();
        let mut late = native_result();
        late.transcript.utterance_id = "qwen-source-session-allowed".into();
        dependencies
            .publish_native_translation_update("microphone", late)
            .await
            .unwrap();
        let rows = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        assert!(rows[0].translations.is_empty());
        while let Ok(event) = events.try_recv() {
            assert!(!event.message_id.contains("blocked") && !event.message_id.contains("unknown"));
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
    async fn late_player_name_updates_saved_and_inflight_results_without_crossing_players() {
        use crate::models::SpeakerIdentity;
        let hub = crate::domain_events::DomainEventHub::new();
        let base = super::super::tests::test_dependencies(hub);
        let mut speaker = SpeakerIdentity {
            id: "vrc-actor".into(),
            index: 0,
            display_name: None,
            user_id: None,
        };
        let first = base.clone().with_player(speaker.clone());
        first
            .publish_text(
                "Before name".into(),
                None,
                "speaker",
                first.scope_id("before"),
            )
            .await
            .unwrap();
        // A retiring session and a new ASR task share late identity metadata.
        let replacement = base.clone().with_player(speaker.clone());
        let other = base.clone().with_player(SpeakerIdentity {
            id: "other-actor".into(),
            index: 1,
            display_name: None,
            user_id: None,
        });
        speaker.display_name = Some("VRC 玩家".into());
        base.publish_speaker(speaker.clone()).await;
        assert_eq!(first.player_identity(), Some(speaker.clone()));
        assert_eq!(replacement.player_identity(), Some(speaker.clone()));
        assert_eq!(other.player_identity().unwrap().display_name, None);
        first
            .publish_text(
                "Finishing retired task".into(),
                None,
                "speaker",
                first.scope_id("after"),
            )
            .await
            .unwrap();
        let mut native = native_result();
        native.transcript.speaker = Some(SpeakerIdentity {
            display_name: None,
            user_id: None,
            ..speaker.clone()
        });
        first
            .publish_native_translation("speaker", native)
            .await
            .unwrap();
        let database = base.database.lock().unwrap();
        let rows = database.subtitle_history(10).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows
            .iter()
            .all(|row| row.speaker.as_ref() == Some(&speaker)));
    }

    #[tokio::test]
    async fn player_sessions_scope_native_and_plain_results_and_reset_only_their_previews() {
        use crate::asr::CloudEvent;
        use crate::models::{LiveTranscription, SpeakerIdentity};
        let hub = crate::domain_events::DomainEventHub::new();
        let mut events = hub.subscribe();
        let base = super::super::tests::test_dependencies(hub);
        let first = base.clone().with_player(SpeakerIdentity {
            id: "actor-1".into(),
            index: 0,
            display_name: None,
            user_id: None,
        });
        let second = base.clone().with_player(SpeakerIdentity {
            id: "actor-2".into(),
            index: 1,
            display_name: None,
            user_id: None,
        });
        assert_ne!(first.scope_id("sentence-1"), second.scope_id("sentence-1"));
        for player in [&first, &second] {
            while events.try_recv().is_ok() {}
            player.publish_live(LiveTranscription::Partial {
                utterance_id: player.scope_id("sentence-1"),
                source: "speaker".into(),
                text: "hello".into(),
                language: Some("en".into()),
            });
            let event = events.try_recv().unwrap();
            assert_eq!(
                event.payload["speaker"]["id"],
                player.player_identity().unwrap().id
            );
            player
                .publish_text(
                    "hello".into(),
                    Some("en".into()),
                    "speaker",
                    player.scope_id("sentence-1"),
                )
                .await
                .unwrap();
        }
        for service in [
            "qwen_live_translate",
            "openai_live_translate",
            "gemini_live_translate",
        ] {
            for player in [&first, &second] {
                let mut result = native_result();
                result.transcript.speaker = Some(SpeakerIdentity {
                    id: "provider-diarized-speaker".into(),
                    index: 99,
                    display_name: None,
                    user_id: None,
                });
                let event = player.scope_event(
                    CloudEvent::LiveTranslation {
                        service: service.into(),
                        snapshot: Box::new(result.transcript.clone()),
                        completed: vec![result],
                        translations: vec![],
                        finished_preview_ids: vec!["old-preview".into()],
                    },
                    "speaker",
                );
                let CloudEvent::LiveTranslation {
                    snapshot,
                    completed,
                    finished_preview_ids,
                    ..
                } = event
                else {
                    unreachable!()
                };
                assert_eq!(snapshot.utterance_id, player.scope_id("native-1"));
                assert_eq!(
                    completed[0].source_utterance_ids,
                    [player.scope_id("native-1")]
                );
                assert_eq!(finished_preview_ids, [player.scope_id("old-preview")]);
                assert_eq!(
                    snapshot.speaker.as_ref().unwrap().id,
                    player.player_identity().unwrap().id
                );
                player.publish_live_translation("speaker", *snapshot);
                player
                    .publish_native_translation("speaker", completed[0].clone())
                    .await
                    .unwrap();
            }
        }
        let rows = base.database.lock().unwrap().subtitle_history(20).unwrap();
        assert_eq!(rows.len(), 8);
        for id in ["actor-1", "actor-2"] {
            let own: Vec<_> = rows
                .iter()
                .filter(|r| r.speaker.as_ref().unwrap().id == id)
                .collect();
            assert_eq!(own.len(), 4);
            assert_eq!(own.iter().filter(|s| !s.translations.is_empty()).count(), 3);
        }
        // A failed continuous session must not clear the other player's preview.
        while events.try_recv().is_ok() {}
        for player in [&first, &second] {
            let mut preview = native_result().transcript;
            preview.utterance_id = player.scope_id("preview-2");
            player.publish_live_translation("speaker", preview);
        }
        while events.try_recv().is_ok() {}
        first.reset_recognition("speaker");
        assert!(!second.player_previews.lock().unwrap().is_empty());
        while let Ok(event) = events.try_recv() {
            assert_ne!(event.event_type, "asr.reset");
            assert_eq!(event.message_id, first.scope_id("preview-2"));
        }
    }

    #[tokio::test]
    async fn twelve_player_pcm_streams_reach_independent_asr_and_translation_requests() {
        use super::super::{run, AsrEchoGuard, PipelineRunContext};
        use crate::config::{ApiAuthMode, ApiProfile, AsrConfig};
        use crate::models::SpeakerIdentity;
        use axum::{body::Bytes, routing::post, Json, Router};
        use serde_json::{json, Value};
        use std::sync::{atomic::AtomicBool, Arc};
        use std::time::Duration;
        use tokio::sync::{mpsc, watch};
        let router = Router::new()
            .route(
                "/v1/audio/transcriptions",
                post(|body: Bytes| async move {
                    let at = body.windows(4).position(|v| v == b"RIFF").unwrap();
                    let length =
                        u32::from_le_bytes(body[at + 4..at + 8].try_into().unwrap()) as usize + 8;
                    let mut wav =
                        hound::WavReader::new(std::io::Cursor::new(&body[at..at + length]))
                            .unwrap();
                    assert_eq!(wav.spec().sample_rate, 16_000);
                    assert_eq!(wav.spec().channels, 1);
                    let samples: Vec<_> = wav.samples::<i16>().map(Result::unwrap).collect();
                    let positive = samples.iter().any(|s| *s > 1000);
                    let negative = samples.iter().any(|s| *s < -1000);
                    assert_ne!(positive, negative, "player PCM must never mix");
                    Json(json!({"text": if positive { "alpha" } else { "beta" }, "language": "en"}))
                }),
            )
            .route(
                "/v1/chat/completions",
                post(|Json(body): Json<Value>| async move {
                    let text = body["messages"].as_array().unwrap().last().unwrap()["content"]
                        .as_str()
                        .unwrap();
                    assert!(text.contains("alpha") ^ text.contains("beta"));
                    let translation = if text.contains("alpha") { "甲" } else { "乙" };
                    let delta = json!({"choices":[{"delta":{"content":translation}}]});
                    (
                        [("content-type", "text/event-stream")],
                        format!("data: {delta}\n\ndata: [DONE]\n\n"),
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let hub = crate::domain_events::DomainEventHub::new();
        let mut events = hub.subscribe();
        let base = super::super::tests::test_dependencies(hub);
        let recognizer = ApiProfile {
            id: "recognizer".into(),
            provider: crate::providers::QWEN_LOCAL_PROVIDER.into(),
            base_url: Some(url.clone()),
            auth_mode: ApiAuthMode::None,
            is_local: true,
            enabled_capabilities: vec![crate::providers::CAPABILITY_SPEECH_TO_TEXT.into()],
            ..Default::default()
        };
        let translator = ApiProfile {
            id: "translator".into(),
            provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(url),
            auth_mode: ApiAuthMode::None,
            is_local: true,
            enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
            ..Default::default()
        };
        let asr = AsrConfig {
            backend: crate::providers::SERVICE_QWEN_LOCAL_TRANSCRIPTION.into(),
            active_profile_id: Some(recognizer.id.clone()),
            api_profiles: vec![recognizer, translator],
            ..Default::default()
        };
        {
            let mut config = base.config.write().unwrap();
            config.asr = asr.clone();
            config.translation.mode = "automatic".into();
            let mut target = TranslationTargetConfig::new("zh-Hans");
            target.profile_id = Some("translator".into());
            target.model = "test-translator".into();
            config.translation.speaker_targets = vec![target];
        }
        let (shutdown, _) = watch::channel(false);
        let (stop, _) = watch::channel(false);
        let mut tasks = Vec::new();
        let mut audio_senders = Vec::new();
        for index in 0..12 {
            let amplitude = if index % 2 == 0 { 0.1f32 } else { -0.2 };
            let dependencies = base.clone().with_player(SpeakerIdentity {
                id: format!("actor-{index}"),
                index: index as u64,
                display_name: None,
                user_id: None,
            });
            let session = crate::asr::spawn_cloud_recognition_session(asr.clone(), 0.2)
                .await
                .unwrap();
            let (sender, receiver) = mpsc::channel(64);
            for frame in 0..36 {
                sender
                    .send(vec![
                        if (4..24).contains(&frame) {
                            amplitude
                        } else {
                            0.0
                        };
                        512
                    ])
                    .await
                    .unwrap();
            }
            audio_senders.push(sender);
            let mut shutdown_rx = shutdown.subscribe();
            let stop_rx = stop.subscribe();
            tasks.push(tokio::spawn(async move {
                let mut capture =
                    crate::audio::AudioCapture::from_stream(receiver, Default::default());
                run(
                    &mut capture,
                    crate::vad::VoiceDetector::default(),
                    crate::vad::SpeechSegmenter::new(16_000, 0.2, 6.0),
                    PipelineRunContext {
                        dependencies,
                        source: "speaker",
                        cloud: Some(session),
                        echo_guard: AsrEchoGuard::default(),
                        sample_rate: 16_000,
                        trigger_threshold_dbfs: Some(-45.0),
                        discard_on_stop: Arc::new(AtomicBool::new(false)),
                        smart_turn: None,
                    },
                    &mut shutdown_rx,
                    stop_rx,
                )
                .await
                .unwrap();
            }));
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut translated = 0;
            while translated < 12 {
                let event = events.recv().await.unwrap();
                assert_ne!(event.event_type, "asr.failed");
                assert_ne!(event.event_type, "translation.failed");
                if event.event_type == "translation.completed" {
                    translated += 1;
                }
            }
            stop.send(true).unwrap();
            for task in tasks {
                task.await.unwrap();
            }
        })
        .await
        .unwrap();
        drop(audio_senders);
        let rows = base.database.lock().unwrap().subtitle_history(20).unwrap();
        assert_eq!(rows.len(), 12);
        for row in rows {
            let first = row.speaker.as_ref().unwrap().index % 2 == 0;
            assert_eq!(row.text, if first { "alpha" } else { "beta" });
            assert_eq!(row.translations[0].text, if first { "甲" } else { "乙" });
        }
        server.abort();
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
                        source_utterance_id: None,
                        completed_original: None,
                        conversation_preview: None,
                        speaker: None,
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
    #[tokio::test]
    async fn same_language_native_source_is_stored_and_published_without_translation_waiting() {
        let dependencies =
            super::super::tests::test_dependencies(crate::domain_events::DomainEventHub::new());
        let mut presentations = dependencies.output.subscribe_presentation_events();
        let mut translations = dependencies.output.subscribe_translations();
        let mut result = native_result();
        result.transcript.language = Some("zh".into());
        result.transcript.target_language = "zh-Hant".into();
        result.transcript.text = "你好。".into();
        result.transcript.translation.clear();
        dependencies
            .publish_native_translation("microphone", result)
            .await
            .unwrap();
        let history = dependencies
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        assert_eq!(history[0].text, "你好。");
        assert!(history[0].translations.is_empty());
        assert!(
            matches!(presentations.try_recv().unwrap(),crate::subtitle_output::PresentationEvent::Final{subtitle,..} if subtitle.text=="你好。")
        );
        assert!(translations.try_recv().is_err());
    }
    #[tokio::test]
    async fn native_speaker_metadata_is_saved_with_source_and_translation() {
        let dependencies =
            super::super::tests::test_dependencies(crate::domain_events::DomainEventHub::new());
        let mut result = native_result();
        let speaker = crate::models::SpeakerIdentity {
            id: "qwen-session-0".into(),
            index: 0,
            display_name: None,
            user_id: None,
        };
        result.transcript.speaker = Some(speaker.clone());
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
        assert_eq!(history[0].speaker.as_ref(), Some(&speaker));
        assert_eq!(history[0].translations[0].text, "你好。");
    }
}
