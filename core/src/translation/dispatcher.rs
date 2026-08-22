use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc, Semaphore};

use crate::config::{ApiProfile, TranslationPromptConfig, TranslationTargetConfig};
use crate::db::conversations::{publish_latest_catalog, ConversationCatalog};
use crate::db::Database;
use crate::models::Subtitle;
use crate::subtitle_output::{SubtitleLifecyclePublisher, TranslationFailure};

use super::TranslationService;

#[derive(Clone)]
pub struct TranslationDispatcher {
    sender: mpsc::Sender<TranslationJob>,
}

#[derive(Clone)]
struct TranslationJob {
    subtitle: Subtitle,
    message_id: String,
    targets: Vec<TranslationTargetConfig>,
    prompt: TranslationPromptConfig,
    profiles: Vec<ApiProfile>,
    include_vrcx_context: bool,
    queued_at: Instant,
}

#[derive(Clone)]
struct TranslationJobContext {
    service: Arc<TranslationService>,
    database: Arc<Mutex<Database>>,
    conversation_catalog: broadcast::Sender<ConversationCatalog>,
    output: SubtitleLifecyclePublisher,
    vrcx: crate::vrcx::VrcxIntegration,
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
        let context = TranslationJobContext {
            service,
            database,
            conversation_catalog,
            output,
            vrcx,
        };
        tokio::spawn(async move {
            let concurrency = Arc::new(Semaphore::new(4));
            while let Some(job) = receiver.recv().await {
                let mut permits = Vec::with_capacity(job.targets.len());
                for _ in &job.targets {
                    let permit = Arc::clone(&concurrency).acquire_owned().await;
                    let Ok(permit) = permit else { return };
                    permits.push(permit);
                }

                // Start every target for one subtitle together so a language cannot
                // advance to newer subtitles while its sibling targets are waiting.
                for ((index, target), permit) in
                    job.targets.iter().cloned().enumerate().zip(permits)
                {
                    let context = context.clone();
                    let job = job.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        process_job(context, job, target, index == 0).await;
                    });
                }
            }
        });
        Self { sender }
    }

    pub fn enqueue(
        &self,
        subtitle: Subtitle,
        targets: Vec<TranslationTargetConfig>,
        prompt: TranslationPromptConfig,
        profiles: Vec<ApiProfile>,
        message_id: String,
        include_vrcx_context: bool,
    ) -> Result<(), String> {
        self.sender
            .try_send(TranslationJob {
                subtitle,
                message_id,
                targets,
                prompt,
                profiles,
                include_vrcx_context,
                queued_at: Instant::now(),
            })
            .map_err(|_| "Translation queue is full".to_string())
    }
}

async fn process_job(
    context: TranslationJobContext,
    job: TranslationJob,
    target: TranslationTargetConfig,
    preferred: bool,
) {
    let TranslationJobContext {
        service,
        database,
        conversation_catalog,
        output,
        vrcx,
    } = context;
    let Some(subtitle_id) = job.subtitle.id else {
        return;
    };
    let queue_wait_ms = job.queued_at.elapsed().as_millis() as u64;
    let started = Instant::now();
    let source = job.subtitle.source.clone();
    output.translation_started_with_message(
        subtitle_id,
        &target.target_language,
        preferred,
        &job.message_id,
        &source,
    );
    let progress_output = output.clone();
    let progress_message_id = job.message_id.clone();
    let progress_source = source.clone();
    let target_language = target.target_language.clone();
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
            preferred,
            &progress_message_id,
            &progress_source,
        );
    };
    let mut context = match database.lock() {
        Ok(database) => database
            .recent_translation_context(&job.prompt, Some(subtitle_id))
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
            &target,
            &job.prompt,
            &job.profiles,
            &job.subtitle.text,
            job.subtitle.language.as_deref(),
            &context,
            Some(&progress),
        )
        .await;
    let result = match first {
        Err(error) if error.retryable => {
            tokio::time::sleep(Duration::from_millis(250)).await;
            service
                .translate_with_progress(
                    &target,
                    &job.prompt,
                    &job.profiles,
                    &job.subtitle.text,
                    job.subtitle.language.as_deref(),
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
                        preferred,
                        &job.message_id,
                        &source,
                    );
                }
                Ok(Err(detail)) => {
                    output.translation_failed_with_message(TranslationFailure {
                        subtitle_id,
                        code: "translation.storage_failed".into(),
                        detail,
                        target_language: &target.target_language,
                        preferred,
                        message_id: &job.message_id,
                        source: &source,
                    });
                }
                Err(error) => {
                    output.translation_failed_with_message(TranslationFailure {
                        subtitle_id,
                        code: "translation.storage_failed".into(),
                        detail: error.to_string(),
                        target_language: &target.target_language,
                        preferred,
                        message_id: &job.message_id,
                        source: &source,
                    });
                }
            }
        }
        Err(error) => {
            output.translation_failed_with_message(TranslationFailure {
                subtitle_id,
                code: error.code.into(),
                detail: error.detail,
                target_language: &target.target_language,
                preferred,
                message_id: &job.message_id,
                source: &source,
            });
        }
    }
}
