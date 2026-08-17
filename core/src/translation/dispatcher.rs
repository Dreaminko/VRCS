use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc, Semaphore};

use crate::config::{ApiProfile, TranslationConfig};
use crate::db::conversations::{publish_latest_catalog, ConversationCatalog};
use crate::db::Database;
use crate::models::Subtitle;
use crate::subtitle_output::SubtitleLifecyclePublisher;

use super::TranslationService;

#[derive(Clone)]
pub struct TranslationDispatcher {
    sender: mpsc::Sender<TranslationJob>,
}

#[derive(Clone)]
struct TranslationJob {
    subtitle: Subtitle,
    message_id: String,
    settings: TranslationConfig,
    profiles: Vec<ApiProfile>,
    include_vrcx_context: bool,
    queued_at: Instant,
}

impl TranslationDispatcher {
    pub fn new(
        service: Arc<TranslationService>,
        database: Arc<Mutex<Database>>,
        conversation_catalog: broadcast::Sender<ConversationCatalog>,
        output: SubtitleLifecyclePublisher,
        vrcx: crate::vrcx::VrcxIntegration,
    ) -> Self {
        let (sender, mut receiver) = mpsc::channel::<TranslationJob>(64);
        tokio::spawn(async move {
            let concurrency = Arc::new(Semaphore::new(4));
            while let Some(job) = receiver.recv().await {
                let permit = Arc::clone(&concurrency).acquire_owned().await;
                let Ok(permit) = permit else { break };
                let service = Arc::clone(&service);
                let database = Arc::clone(&database);
                let conversation_catalog = conversation_catalog.clone();
                let output = output.clone();
                let vrcx = vrcx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    process_job(service, database, conversation_catalog, output, vrcx, job).await;
                });
            }
        });
        Self { sender }
    }

    pub fn enqueue(
        &self,
        subtitle: Subtitle,
        settings: TranslationConfig,
        profiles: Vec<ApiProfile>,
        message_id: String,
        include_vrcx_context: bool,
    ) -> Result<(), String> {
        self.sender
            .try_send(TranslationJob {
                subtitle,
                message_id,
                settings,
                profiles,
                include_vrcx_context,
                queued_at: Instant::now(),
            })
            .map_err(|_| "Translation queue is full".to_string())
    }
}

async fn process_job(
    service: Arc<TranslationService>,
    database: Arc<Mutex<Database>>,
    conversation_catalog: broadcast::Sender<ConversationCatalog>,
    output: SubtitleLifecyclePublisher,
    vrcx: crate::vrcx::VrcxIntegration,
    job: TranslationJob,
) {
    let Some(subtitle_id) = job.subtitle.id else {
        return;
    };
    let queue_wait_ms = job.queued_at.elapsed().as_millis() as u64;
    let started = Instant::now();
    let source = job.subtitle.source.clone();
    output.translation_started_with_message(subtitle_id, &job.message_id, &source);
    let progress_output = output.clone();
    let progress_message_id = job.message_id.clone();
    let progress_source = source.clone();
    let target_language = job.settings.target_language.clone();
    let last_progress = Mutex::new(Instant::now() - Duration::from_millis(80));
    let progress = move |text: &str| {
        let Ok(mut last) = last_progress.lock() else {
            return;
        };
        if last.elapsed() < Duration::from_millis(80) {
            return;
        }
        *last = Instant::now();
        progress_output.translation_partial_with_message(
            subtitle_id,
            text.to_owned(),
            target_language.clone(),
            &progress_message_id,
            &progress_source,
        );
    };
    let mut context = match database.lock() {
        Ok(database) => database
            .recent_translation_context(&job.settings.prompt, Some(subtitle_id))
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "translation context could not be loaded");
                Vec::new()
            }),
        Err(_) => {
            tracing::warn!("translation context database lock is unavailable");
            Vec::new()
        }
    };
    if job.include_vrcx_context {
        if let Some(entry) = vrcx.translation_context_entry() {
            context.push(entry);
        }
    }
    let first = service
        .translate_with_progress(
            &job.settings,
            &job.profiles,
            &job.subtitle.text,
            job.subtitle.language.as_deref(),
            None,
            &context,
            Some(&progress),
        )
        .await;
    let result = match first {
        Err(error) if error.retryable => {
            tokio::time::sleep(Duration::from_millis(250)).await;
            service
                .translate_with_progress(
                    &job.settings,
                    &job.profiles,
                    &job.subtitle.text,
                    job.subtitle.language.as_deref(),
                    None,
                    &context,
                    Some(&progress),
                )
                .await
        }
        other => other,
    };
    tracing::info!(
        subtitle_id,
        queue_wait_ms,
        total_ms = started.elapsed().as_millis() as u64,
        success = result.is_ok(),
        "translation job completed"
    );
    match result {
        Ok(result) => {
            let record = result.into_record();
            let stored = tokio::task::spawn_blocking({
                let database = Arc::clone(&database);
                let record = record.clone();
                move || {
                    let database = database
                        .lock()
                        .map_err(|_| "Database lock is unavailable".to_string())?;
                    let catalog_changed = database
                        .save_translation(subtitle_id, &record)
                        .map_err(|error| error.to_string())?;
                    if catalog_changed {
                        publish_latest_catalog(&database, &conversation_catalog);
                    }
                    Ok::<_, String>(())
                }
            })
            .await;
            match stored {
                Ok(Ok(())) => {
                    output.translation_completed_with_message(
                        subtitle_id,
                        record,
                        &job.message_id,
                        &source,
                    );
                }
                Ok(Err(detail)) => {
                    output.translation_failed_with_message(
                        subtitle_id,
                        "translation.storage_failed".into(),
                        detail,
                        &job.message_id,
                        &source,
                    );
                }
                Err(error) => {
                    output.translation_failed_with_message(
                        subtitle_id,
                        "translation.storage_failed".into(),
                        error.to_string(),
                        &job.message_id,
                        &source,
                    );
                }
            }
        }
        Err(error) => {
            output.translation_failed_with_message(
                subtitle_id,
                error.code.into(),
                error.detail,
                &job.message_id,
                &source,
            );
        }
    }
}
