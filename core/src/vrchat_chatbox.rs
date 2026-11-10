#[cfg(any(windows, test))]
use std::sync::Mutex;
use std::sync::{Arc, RwLock};

#[cfg(any(windows, test))]
use tokio::sync::broadcast;
#[cfg(windows)]
use tokio::sync::mpsc;
use tokio::sync::watch;

#[cfg(any(windows, test))]
use crate::config::AppConfig;
#[cfg(any(windows, test))]
use crate::db::conversations::{CatalogRefresh, ConversationCatalog};
#[cfg(any(windows, test))]
use crate::db::Database;
#[cfg(any(windows, test))]
use crate::language_session::ActiveLanguageSession;
#[cfg(any(windows, test))]
use crate::models::{SpeakerIdentity, Subtitle};
#[cfg(any(windows, test))]
use crate::subtitle_output::{SubtitleLifecyclePublisher, TranslationFailure};
#[cfg(any(windows, test))]
use crate::translation::TranslationDispatcher;

#[derive(Debug, Clone)]
pub(crate) struct TaskValidity {
    current: Arc<RwLock<bool>>,
    cancelled: watch::Sender<bool>,
}

impl TaskValidity {
    pub(crate) fn new() -> Self {
        Self {
            current: Arc::new(RwLock::new(true)),
            cancelled: watch::channel(false).0,
        }
    }

    pub(crate) fn invalidate(&self) {
        // The write lock waits for an active storage/publication boundary.
        if let Ok(mut current) = self.current.write() {
            *current = false;
        }
        self.cancelled.send_replace(true);
    }

    pub(crate) fn is_current(&self) -> bool {
        self.current.read().is_ok_and(|current| *current)
    }

    pub(crate) fn with_current<T>(&self, action: impl FnOnce() -> T) -> Option<T> {
        let current = self.current.read().ok()?;
        (*current).then(action)
    }

    pub(crate) async fn cancelled(&self) {
        let mut receiver = self.cancelled.subscribe();
        loop {
            if *receiver.borrow_and_update() || !self.is_current() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

#[cfg(any(windows, test))]
pub(crate) struct ChatboxMessage {
    pub text: String,
    pub speaker: SpeakerIdentity,
    pub received_at: String,
    pub validity: TaskValidity,
}

pub(crate) enum ChatboxEvent {
    #[cfg(windows)]
    Message(ChatboxMessage),
    #[cfg(any(windows, test))]
    Speaker(SpeakerIdentity),
}

#[cfg(any(windows, test))]
pub(crate) struct ChatboxConsumer {
    database: Arc<Mutex<Database>>,
    catalog: CatalogRefresh,
    output: SubtitleLifecyclePublisher,
    dispatcher: TranslationDispatcher,
    config: Arc<RwLock<AppConfig>>,
    language_session: Arc<RwLock<ActiveLanguageSession>>,
}

#[cfg(any(windows, test))]
impl ChatboxConsumer {
    pub(crate) fn new(
        database: Arc<Mutex<Database>>,
        conversation_catalog: broadcast::Sender<ConversationCatalog>,
        output: SubtitleLifecyclePublisher,
        dispatcher: TranslationDispatcher,
        config: Arc<RwLock<AppConfig>>,
        language_session: Arc<RwLock<ActiveLanguageSession>>,
    ) -> Self {
        Self {
            catalog: CatalogRefresh::new(Arc::clone(&database), conversation_catalog),
            database,
            output,
            dispatcher,
            config,
            language_session,
        }
    }

    #[cfg(windows)]
    pub(crate) async fn run(self, mut receiver: mpsc::Receiver<ChatboxEvent>) {
        while let Some(event) = receiver.recv().await {
            let result = match event {
                ChatboxEvent::Message(message) => self.record(message).await,
                ChatboxEvent::Speaker(speaker) => self.update_speaker(speaker).await,
            };
            if let Err(error) = result {
                tracing::warn!(%error, "VRChat Chatbox subtitle processing failed");
            }
        }
    }

    async fn update_speaker(&self, speaker: SpeakerIdentity) -> Result<(), String> {
        let database = Arc::clone(&self.database);
        let update = speaker.clone();
        tokio::task::spawn_blocking(move || {
            database
                .lock()
                .map_err(|_| "Database lock is unavailable".to_string())?
                .update_speaker_identity(&update)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())??;
        self.output.speaker_updated(&speaker);
        Ok(())
    }

    async fn record(&self, message: ChatboxMessage) -> Result<(), String> {
        if message.text.trim().is_empty() || message.text.chars().count() > 5_000 {
            return Ok(());
        }
        let message_id = format!("vrchat-chatbox:{}", uuid::Uuid::new_v4());
        let validity = message.validity;
        let subtitle = Subtitle {
            id: None,
            conversation_id: None,
            speaker: Some(message.speaker),
            text: message.text,
            language: None,
            started_at: None,
            ended_at: None,
            source: "vrchat_chatbox".into(),
            created_at: message.received_at,
            translations: Vec::new(),
        };
        let saved = tokio::task::spawn_blocking({
            let database = Arc::clone(&self.database);
            let validity = validity.clone();
            move || {
                // Do not hold validity while waiting for the database lock.
                let database = database
                    .lock()
                    .map_err(|_| "Database lock is unavailable".to_string())?;
                validity
                    .with_current(|| {
                        database
                            .add_subtitle(&subtitle)
                            .map_err(|error| error.to_string())
                    })
                    .transpose()
            }
        })
        .await
        .map_err(|error| error.to_string())??;
        let Some(saved) = saved else { return Ok(()) };
        if validity
            .with_current(|| {
                self.catalog.request();
                self.output
                    .remote_subtitle_recorded(saved.clone(), &message_id);
            })
            .is_none()
        {
            return Ok(());
        }
        let (translation, profiles, include_vrcx_context) = {
            let config = self
                .config
                .read()
                .map_err(|_| "Config lock is unavailable".to_string())?;
            let language = self
                .language_session
                .read()
                .map_err(|_| "Language session lock is unavailable".to_string())?
                .resolve(&config);
            (
                language.translation,
                config.asr.api_profiles.clone(),
                config.vrcx.enabled && config.vrcx.include_in_llm_context,
            )
        };
        if translation.mode != "automatic" || translation.speaker_targets.is_empty() {
            return Ok(());
        }
        let targets = translation.speaker_targets;
        let failed_targets = targets.clone();
        if let Err(detail) = self.dispatcher.enqueue_guarded(
            saved.clone(),
            targets,
            translation.prompt,
            profiles,
            message_id.clone(),
            include_vrcx_context,
            true,
            validity.clone(),
        ) {
            validity.with_current(|| {
                for (index, target) in failed_targets.iter().enumerate() {
                    self.output
                        .translation_failed_with_message(TranslationFailure {
                            subtitle_id: saved.id.expect("stored subtitle"),
                            code: "translation.queue_full".into(),
                            detail: detail.clone(),
                            target_language: &target.target_language,
                            preferred: index == 0,
                            message_id: &message_id,
                            source: "vrchat_chatbox",
                        });
                }
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiAuthMode, ApiProfile, OscConfig, TranslationTargetConfig};
    use crate::models::now_iso8601;
    use crate::subtitle_output::TranslationEvent;
    use std::time::Duration;

    struct Fixture {
        consumer: ChatboxConsumer,
        database: Arc<Mutex<Database>>,
        _directory: tempfile::TempDir,
    }

    fn fixture(mode: &str) -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let database = Arc::new(Mutex::new(
            Database::open(&directory.path().join("chatbox.db")).unwrap(),
        ));
        let (catalog, _) = broadcast::channel(16);
        let output = SubtitleLifecyclePublisher::new(
            broadcast::channel(16).0,
            broadcast::channel(16).0,
            crate::osc::OscChatboxDispatcher::new(OscConfig::default()),
        );
        let dispatcher = TranslationDispatcher::new(
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::clone(&database),
            catalog.clone(),
            output.clone(),
            crate::vrcx::VrcxIntegration::new(watch::channel(false).1),
        );
        let mut config = AppConfig::default();
        config.translation.mode = mode.into();
        Fixture {
            consumer: ChatboxConsumer::new(
                Arc::clone(&database),
                catalog,
                output,
                dispatcher,
                Arc::new(RwLock::new(config)),
                Arc::new(RwLock::new(ActiveLanguageSession::Global)),
            ),
            database,
            _directory: directory,
        }
    }

    fn message(validity: TaskValidity) -> ChatboxMessage {
        ChatboxMessage {
            text: "  Synthetic 🦊\n第二行  ".into(),
            speaker: SpeakerIdentity {
                id: "vrc:chatbox-scope".into(),
                index: 3,
                display_name: None,
                user_id: None,
            },
            received_at: now_iso8601(),
            validity,
        }
    }

    async fn receive(receiver: &mut broadcast::Receiver<TranslationEvent>) -> TranslationEvent {
        tokio::time::timeout(Duration::from_secs(3), receiver.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn original_is_preserved_in_all_modes_and_missing_service_reports_failure() {
        for mode in ["disabled", "manual", "automatic"] {
            let fixture = fixture(mode);
            let mut subtitles = fixture.consumer.output.subscribe_subtitles();
            let mut translations = fixture.consumer.output.subscribe_translations();
            let original = message(TaskValidity::new());
            let text = original.text.clone();
            let speaker = original.speaker.clone();
            fixture.consumer.record(original).await.unwrap();
            let subtitle = subtitles.recv().await.unwrap();
            assert_eq!(subtitle.text, text);
            assert_eq!(subtitle.speaker, Some(speaker));
            assert_eq!(subtitle.source, "vrchat_chatbox");
            assert_eq!(subtitle.language, None);
            if mode == "automatic" {
                assert!(matches!(
                    receive(&mut translations).await,
                    TranslationEvent::TranslationStarted { .. }
                ));
                assert!(
                    matches!(receive(&mut translations).await, TranslationEvent::TranslationFailed { code, .. }
                    if code == "translation.not_configured")
                );
            } else {
                assert!(translations.try_recv().is_err());
            }
            let history = fixture
                .database
                .lock()
                .unwrap()
                .subtitle_history(10)
                .unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].text, text);
            assert!(history[0].translations.is_empty());
            let stored =
                rusqlite::Connection::open(fixture._directory.path().join("chatbox.db")).unwrap();
            assert_eq!(
                stored
                    .query_row("SELECT COUNT(*) FROM chatbox_messages", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }

    #[tokio::test]
    async fn invalidated_queued_text_cannot_store_or_publish_and_names_can_be_repaired() {
        let fixture = fixture("disabled");
        let mut subtitles = fixture.consumer.output.subscribe_subtitles();
        let validity = TaskValidity::new();
        validity.invalidate();
        fixture
            .consumer
            .record(message(validity.clone()))
            .await
            .unwrap();
        assert!(!validity.is_current());
        assert_eq!(validity.with_current(|| 1), None);
        tokio::time::timeout(Duration::from_millis(100), validity.cancelled())
            .await
            .unwrap();
        assert!(fixture
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap()
            .is_empty());
        assert!(subtitles.try_recv().is_err());
        fixture
            .consumer
            .record(message(TaskValidity::new()))
            .await
            .unwrap();
        let mut speaker = message(TaskValidity::new()).speaker;
        speaker.display_name = Some("Late synthetic name".into());
        fixture.consumer.update_speaker(speaker).await.unwrap();
        assert_eq!(
            fixture
                .database
                .lock()
                .unwrap()
                .subtitle_history(10)
                .unwrap()[0]
                .speaker
                .as_ref()
                .unwrap()
                .display_name
                .as_deref(),
            Some("Late synthetic name")
        );
    }

    #[tokio::test]
    async fn active_language_session_selects_other_player_targets_and_translates_each_once() {
        use axum::{routing::post, Json, Router};
        use serde_json::{json, Value};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let router = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(body): Json<Value>| {
                let calls = Arc::clone(&observed);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert!(
                        body["messages"].as_array().unwrap().last().unwrap()["content"]
                            .as_str()
                            .unwrap()
                            .contains("Synthetic")
                    );
                    let delta = json!({"choices":[{"delta":{"content":"Synthetic translation"}}]});
                    (
                        [("content-type", "text/event-stream")],
                        format!("data: {delta}\n\ndata: [DONE]\n\n"),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let fixture = fixture("disabled");
        let mut translations = fixture.consumer.output.subscribe_translations();
        {
            let mut config = fixture.consumer.config.write().unwrap();
            config.asr.api_profiles = vec![ApiProfile {
                id: "text-service".into(),
                provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
                base_url: Some(url),
                auth_mode: ApiAuthMode::None,
                is_local: true,
                enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
                ..Default::default()
            }];
            let mut translation = config.translation.clone();
            translation.mode = "automatic".into();
            translation.speaker_targets = ["ja", "fr"]
                .into_iter()
                .map(|language| {
                    let mut target = TranslationTargetConfig::new(language);
                    target.use_default_model = false;
                    target.profile_id = Some("text-service".into());
                    target.model = "test-model".into();
                    target
                })
                .collect();
            *fixture.consumer.language_session.write().unwrap() = ActiveLanguageSession::Override {
                snapshot: crate::language_session::LanguageRuntimeConfig {
                    recognition_language: "en".into(),
                    translation,
                    osc_translation_strategy: "preferred_only".into(),
                },
            };
        }
        fixture
            .consumer
            .record(message(TaskValidity::new()))
            .await
            .unwrap();
        let mut completed = Vec::new();
        while completed.len() < 2 {
            match receive(&mut translations).await {
                TranslationEvent::TranslationCompleted { translation, .. } => {
                    completed.push(translation.target_language)
                }
                TranslationEvent::TranslationFailed { code, detail, .. } => {
                    panic!("{code}: {detail}")
                }
                _ => {}
            }
        }
        completed.sort();
        assert_eq!(completed, ["fr", "ja"]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            fixture
                .database
                .lock()
                .unwrap()
                .subtitle_history(10)
                .unwrap()[0]
                .translations
                .len(),
            2
        );
        server.abort();
    }

    #[tokio::test]
    async fn invalidating_in_flight_http_translation_prevents_late_storage_and_publication() {
        use axum::{routing::post, Router};
        use serde_json::json;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let request_entered = Arc::clone(&entered);
        let request_release = Arc::clone(&release);
        let router = Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let entered = Arc::clone(&request_entered);
                let release = Arc::clone(&request_release);
                async move {
                    entered.notify_one();
                    release.notified().await;
                    let delta = json!({"choices":[{"delta":{"content":"Late translation"}}]});
                    (
                        [("content-type", "text/event-stream")],
                        format!("data: {delta}\n\ndata: [DONE]\n\n"),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let fixture = fixture("automatic");
        {
            let mut config = fixture.consumer.config.write().unwrap();
            config.asr.api_profiles = vec![ApiProfile {
                id: "delayed".into(),
                provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
                base_url: Some(url),
                auth_mode: ApiAuthMode::None,
                is_local: true,
                enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
                ..Default::default()
            }];
            config.translation.speaker_targets[0].use_default_model = false;
            config.translation.speaker_targets[0].profile_id = Some("delayed".into());
        }
        let mut translations = fixture.consumer.output.subscribe_translations();
        let validity = TaskValidity::new();
        fixture
            .consumer
            .record(message(validity.clone()))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        assert!(matches!(
            receive(&mut translations).await,
            TranslationEvent::TranslationStarted { .. }
        ));
        validity.invalidate();
        release.notify_one();
        assert!(
            tokio::time::timeout(Duration::from_millis(150), translations.recv())
                .await
                .is_err()
        );
        let history = fixture
            .database
            .lock()
            .unwrap()
            .subtitle_history(10)
            .unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].translations.is_empty());
        server.abort();
    }
}
