use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;

use crate::config::{
    ApiProfile, TranslationBackend, TranslationPromptConfig, TranslationTargetConfig,
};
use crate::db::conversations::{CatalogRefresh, ConversationCatalog};
use crate::db::Database;
use crate::models::Subtitle;
use crate::subtitle_output::{SubtitleLifecyclePublisher, TranslationFailure};
use crate::vrchat_chatbox::TaskValidity;
use crate::work_priority::Context;

use super::TranslationService;

fn retry_delay(error: &super::TranslationError) -> Option<Duration> {
    if !error.retryable {
        return None;
    }
    let delay = error.retry_after.unwrap_or(Duration::from_millis(250));
    (delay <= Duration::from_secs(5)).then_some(delay)
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn respects_server_delays_and_limits_automatic_subtitle_retries() {
        let mut error = super::super::TranslationError {
            code: "llm.rate_limited",
            detail: String::new(),
            retryable: true,
            retry_after: None,
        };
        assert_eq!(retry_delay(&error), Some(Duration::from_millis(250)));
        error.retry_after = Some(Duration::from_secs(3));
        assert_eq!(retry_delay(&error), Some(Duration::from_secs(3)));
        error.retry_after = Some(Duration::from_secs(6));
        assert_eq!(retry_delay(&error), None);
        error.retry_after = None;
        error.retryable = false;
        assert_eq!(retry_delay(&error), None);
    }

    fn queued(id: &str, speaker: &str, count: usize, budget: &Arc<Semaphore>) -> QueuedJob {
        QueuedJob {
            job: TranslationJob {
                subtitle: Subtitle {
                    id: Some(1),
                    conversation_id: None,
                    text: "Translate this sentence".into(),
                    speaker: Some(crate::models::SpeakerIdentity {
                        id: speaker.into(),
                        index: 0,
                        display_name: None,
                        user_id: None,
                    }),
                    language: Some("en".into()),
                    started_at: None,
                    ended_at: None,
                    source: "speaker".into(),
                    created_at: String::new(),
                    translations: Vec::new(),
                },
                message_id: id.into(),
                targets: ["ja", "zh-Hans", "fr"]
                    .into_iter()
                    .take(count)
                    .map(TranslationTargetConfig::new)
                    .collect(),
                prompt: TranslationPromptConfig::default(),
                profiles: Vec::new(),
                include_vrcx_context: false,
                first_preferred: false,
                queued_at: Instant::now(),
                validity: TaskValidity::new(),
            },
            _permit: Arc::clone(budget).try_acquire_owned().unwrap(),
        }
    }

    #[test]
    fn target_weighted_priority_excludes_chatbox_and_reads_latest_view() {
        let budget = Arc::new(Semaphore::new(QUEUE_CAPACITY));
        let mut pending = VecDeque::from([
            queued("high-3", "vrc:viewed", 3, &budget),
            queued("high-1", "vrc:viewed", 1, &budget),
            queued("ordinary", "vrc:other", 1, &budget),
            queued("chatbox", "vrc:viewed", 1, &budget),
        ]);
        pending[3].job.subtitle.source = "vrchat_chatbox".into();
        crate::player_audio::publish_view_priority(
            true,
            HashSet::from(["vrc:viewed".into()]),
            Some(Instant::now() + Duration::from_secs(10)),
        );
        let mut admission = Admission::default();
        let active = HashMap::new();
        let first = admission.next_job(&pending, 4, 10, &active).unwrap();
        assert_eq!(pending.remove(first).unwrap().job.message_id, "high-3");
        let second = admission.next_job(&pending, 4, 10, &active).unwrap();
        assert_eq!(pending.remove(second).unwrap().job.message_id, "ordinary");
        assert!(!job_priority(&pending[1].job).is_priority());
        crate::player_audio::clear_view_priority();
        assert!(!job_priority(&pending[0].job).is_priority());
    }

    #[test]
    fn aged_groups_accumulate_capacity_without_later_player_sentences_passing() {
        let budget = Arc::new(Semaphore::new(QUEUE_CAPACITY));
        let mut pending = VecDeque::from([
            queued("older", "vrc:a", 3, &budget),
            queued("later", "vrc:a", 1, &budget),
            queued("other", "vrc:b", 1, &budget),
        ]);
        let mut admission = Admission::default();
        let active = HashMap::new();
        assert_eq!(admission.next_job(&pending, 1, 10, &active), Some(2));
        pending[0].job.queued_at = Instant::now() - STARVATION;
        assert_eq!(admission.next_job(&pending, 2, 10, &active), None);
        assert_eq!(admission.next_job(&pending, 3, 10, &active), Some(0));
        pending.remove(0);
        let active = HashMap::from([("vrc:a".into(), 3)]);
        assert_eq!(admission.next_job(&pending, 4, 10, &active), Some(1));
        pending[0].job.targets = ["ja", "zh-Hans", "fr"]
            .into_iter()
            .map(|language| {
                let mut target = TranslationTargetConfig::new(language);
                target.backend = TranslationBackend::PlamoLocal;
                target
            })
            .collect();
        assert_eq!(resources(&pending[0].job), (0, 3));
        assert_eq!(admission.next_job(&pending, 0, 3, &HashMap::new()), Some(0));
    }

    #[tokio::test]
    async fn full_queue_reports_backpressure_without_enqueuing_invalidated_work() {
        let (sender, mut receiver) = mpsc::channel(QUEUE_CAPACITY);
        let dispatcher = TranslationDispatcher {
            sender,
            queue_budget: Arc::new(Semaphore::new(QUEUE_CAPACITY)),
        };
        let subtitle = Subtitle {
            id: Some(1),
            conversation_id: None,
            speaker: None,
            text: "Synthetic remote text".into(),
            language: None,
            started_at: None,
            ended_at: None,
            source: "vrchat_chatbox".into(),
            created_at: crate::models::now_iso8601(),
            translations: Vec::new(),
        };
        let enqueue = |validity| {
            dispatcher.enqueue_guarded(
                subtitle.clone(),
                vec![TranslationTargetConfig::new("ja")],
                TranslationPromptConfig::default(),
                Vec::new(),
                "synthetic-message".into(),
                false,
                true,
                validity,
            )
        };
        let mut pending = VecDeque::new();
        for _ in 0..QUEUE_CAPACITY {
            assert!(enqueue(TaskValidity::new()).is_ok());
            pending.push_back(receiver.try_recv().unwrap());
        }
        assert!(enqueue(TaskValidity::new()).is_err());
        pending.pop_front();
        assert!(enqueue(TaskValidity::new()).is_ok());
        let cancelled = TaskValidity::new();
        cancelled.invalidate();
        assert!(enqueue(cancelled).is_ok());
    }
}

#[derive(Clone)]
pub struct TranslationDispatcher {
    sender: mpsc::Sender<QueuedJob>,
    queue_budget: Arc<Semaphore>,
}

struct QueuedJob {
    job: TranslationJob,
    _permit: OwnedSemaphorePermit,
}

#[derive(Clone)]
struct TranslationJob {
    subtitle: Subtitle,
    message_id: String,
    targets: Vec<TranslationTargetConfig>,
    prompt: TranslationPromptConfig,
    profiles: Vec<ApiProfile>,
    include_vrcx_context: bool,
    first_preferred: bool,
    queued_at: Instant,
    validity: TaskValidity,
}

#[derive(Clone)]
struct TranslationJobContext {
    service: Arc<TranslationService>,
    database: Arc<Mutex<Database>>,
    catalog: CatalogRefresh,
    output: SubtitleLifecyclePublisher,
    vrcx: crate::vrcx::VrcxIntegration,
}

/// Resource-weighted 3:1 admission, with bounded aging reservations.
const PRIORITY_BURST: i32 = 3;
const STARVATION: Duration = Duration::from_secs(2);
const QUEUE_CAPACITY: usize = 64;
const REMOTE_CAPACITY: usize = 4;
// Independent of remote requests: two running local requests and eight waiters.
const LOCAL_CAPACITY: usize = 10;

fn player(job: &TranslationJob) -> Option<&str> {
    (job.subtitle.source == "speaker")
        .then_some(job.subtitle.speaker.as_ref())
        .flatten()
        .map(|speaker| speaker.id.as_str())
}

fn job_priority(job: &TranslationJob) -> Context {
    player(job).map(Context::for_player).unwrap_or_default()
}

fn resources(job: &TranslationJob) -> (usize, usize) {
    let local = job
        .targets
        .iter()
        .filter(|target| target.backend == TranslationBackend::PlamoLocal)
        .count();
    (job.targets.len() - local, local)
}

#[derive(Default)]
struct Admission {
    priority_debt: i32,
    last_priority: Option<String>,
    last_ordinary: Option<String>,
}

impl Admission {
    fn next_job(
        &mut self,
        pending: &VecDeque<QueuedJob>,
        remote_available: usize,
        local_available: usize,
        active_players: &HashMap<String, usize>,
    ) -> Option<usize> {
        let eligible = |index: usize| {
            player(&pending[index].job).is_none_or(|id| {
                !active_players.contains_key(id)
                    && !pending
                        .iter()
                        .take(index)
                        .any(|queued| player(&queued.job) == Some(id))
            })
        };
        let fits = |index: usize| {
            let (remote, local) = resources(&pending[index].job);
            remote <= remote_available && local <= local_available
        };
        let aged = (0..pending.len())
            .filter(|&index| eligible(index))
            .filter(|&index| pending[index].job.queued_at.elapsed() >= STARVATION)
            .min_by_key(|&index| pending[index].job.queued_at);
        let ready = |index: usize| {
            if !fits(index) {
                return false;
            }
            // Accumulate only the resources needed by the oldest aged group;
            // independent local/remote work can still proceed.
            aged.is_none_or(|oldest| {
                let (needed_remote, needed_local) = resources(&pending[oldest].job);
                let (remote, local) = resources(&pending[index].job);
                (needed_remote <= remote_available || remote == 0)
                    && (needed_local <= local_available || local == 0)
            })
        };
        let pick = |priority: bool, last: &Option<String>| {
            let mut matching = (0..pending.len()).filter(|&index| {
                eligible(index)
                    && ready(index)
                    && job_priority(&pending[index].job).is_priority() == priority
            });
            let first = matching.next()?;
            if player(&pending[first].job) != last.as_deref() {
                return Some(first);
            }
            matching
                .find(|&index| player(&pending[index].job) != last.as_deref())
                .or(Some(first))
        };
        let priority = pick(true, &self.last_priority);
        let ordinary = pick(false, &self.last_ordinary);
        let index = aged
            .filter(|&index| ready(index))
            .or(match (priority, ordinary) {
                (Some(index), Some(_)) if self.priority_debt < PRIORITY_BURST => Some(index),
                (_, Some(index)) => Some(index),
                (Some(index), None) => Some(index),
                (None, None) => None,
            })?;
        let job = &pending[index].job;
        let cost = job.targets.len() as i32;
        if job_priority(job).is_priority() {
            self.priority_debt = if ordinary.is_some() {
                self.priority_debt + cost
            } else {
                (self.priority_debt + cost).min(PRIORITY_BURST)
            };
            self.last_priority = player(job).map(str::to_owned);
        } else {
            self.priority_debt = if priority.is_some() {
                self.priority_debt - PRIORITY_BURST * cost
            } else {
                0
            };
            self.last_ordinary = player(job).map(str::to_owned);
        }
        Some(index)
    }
}

impl TranslationDispatcher {
    pub fn new(
        service: Arc<TranslationService>,
        database: Arc<Mutex<Database>>,
        conversation_catalog: broadcast::Sender<ConversationCatalog>,
        output: SubtitleLifecyclePublisher,
        vrcx: crate::vrcx::VrcxIntegration,
    ) -> Self {
        let (sender, mut receiver) = mpsc::channel::<QueuedJob>(QUEUE_CAPACITY);
        let queue_budget = Arc::new(Semaphore::new(QUEUE_CAPACITY));
        let context = TranslationJobContext {
            service,
            catalog: CatalogRefresh::new(Arc::clone(&database), conversation_catalog),
            database,
            output,
            vrcx,
        };
        tokio::spawn(async move {
            let remote = Arc::new(Semaphore::new(REMOTE_CAPACITY));
            let local = Arc::new(Semaphore::new(LOCAL_CAPACITY));
            let mut running = JoinSet::new();
            let mut active_players = HashMap::new();
            let mut task_players = HashMap::new();
            let mut pending = VecDeque::new();
            let mut admission = Admission::default();
            let mut closed = false;
            loop {
                while !closed {
                    match receiver.try_recv() {
                        Ok(job) => pending.push_back(job),
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            closed = true;
                            break;
                        }
                    }
                }
                pending.retain(|queued: &QueuedJob| queued.job.validity.is_current());
                if let Some(index) = admission.next_job(
                    &pending,
                    remote.available_permits(),
                    local.available_permits(),
                    &active_players,
                ) {
                    let QueuedJob { job, _permit } = pending.remove(index).expect("selected job");
                    drop(_permit);
                    // Reserve upstream budgets, never the two real inference slots.
                    let player_id = player(&job).map(str::to_owned);
                    for (index, target) in job.targets.iter().cloned().enumerate() {
                        let budget = if target.backend == TranslationBackend::PlamoLocal {
                            &local
                        } else {
                            &remote
                        };
                        let permit = Arc::clone(budget)
                            .try_acquire_owned()
                            .expect("selected group fits its resource budgets");
                        let context = context.clone();
                        let job = job.clone();
                        let task = running.spawn(async move {
                            let preferred = job.first_preferred && index == 0;
                            job_priority(&job)
                                .scope(process_job(context, job, target, preferred))
                                .await;
                            // Drop also returns capacity on panic, before JoinSet wakes dispatch.
                            drop(permit);
                        });
                        if let Some(id) = &player_id {
                            *active_players.entry(id.clone()).or_insert(0) += 1;
                            task_players.insert(task.id(), id.clone());
                        }
                    }
                    continue;
                }
                if closed && pending.is_empty() && running.is_empty() {
                    break;
                }
                tokio::select! {
                    // Bound cancellation cleanup and re-evaluate view/aging even
                    // when every queue budget is held and no new job can enter.
                    _ = tokio::time::sleep(Duration::from_millis(50)), if !pending.is_empty() => {}
                    result = running.join_next_with_id(), if !running.is_empty() => {
                        let task_id = match result.expect("running task") {
                            Ok((id, ())) => id,
                            Err(error) => error.id(),
                        };
                        if let Some(id) = task_players.remove(&task_id) {
                            if let Some(remaining) = active_players.get_mut(&id) {
                                *remaining -= 1;
                                if *remaining == 0 {
                                    active_players.remove(&id);
                                }
                            }
                        }
                    }
                    job = receiver.recv(), if !closed => match job {
                        Some(job) => pending.push_back(job),
                        None => closed = true,
                    }
                }
            }
        });
        Self {
            sender,
            queue_budget,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue(
        &self,
        subtitle: Subtitle,
        targets: Vec<TranslationTargetConfig>,
        prompt: TranslationPromptConfig,
        profiles: Vec<ApiProfile>,
        message_id: String,
        include_vrcx_context: bool,
        first_preferred: bool,
    ) -> Result<(), String> {
        self.enqueue_guarded(
            subtitle,
            targets,
            prompt,
            profiles,
            message_id,
            include_vrcx_context,
            first_preferred,
            TaskValidity::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn enqueue_guarded(
        &self,
        subtitle: Subtitle,
        targets: Vec<TranslationTargetConfig>,
        prompt: TranslationPromptConfig,
        profiles: Vec<ApiProfile>,
        message_id: String,
        include_vrcx_context: bool,
        first_preferred: bool,
        validity: TaskValidity,
    ) -> Result<(), String> {
        validity
            .with_current(|| {
                let permit = Arc::clone(&self.queue_budget)
                    .try_acquire_owned()
                    .map_err(|_| "Translation queue is full".to_string())?;
                self.sender
                    .try_send(QueuedJob {
                        job: TranslationJob {
                            subtitle,
                            message_id,
                            targets,
                            prompt,
                            profiles,
                            include_vrcx_context,
                            first_preferred,
                            queued_at: Instant::now(),
                            validity: validity.clone(),
                        },
                        _permit: permit,
                    })
                    .map_err(|_| "Translation queue is full".to_string())
            })
            .unwrap_or(Ok(()))
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
        catalog,
        output,
        vrcx,
    } = context;
    let Some(subtitle_id) = job.subtitle.id else {
        return;
    };
    let queue_wait_ms = job.queued_at.elapsed().as_millis() as u64;
    let started = Instant::now();
    let source = job.subtitle.source.clone();
    if !job.validity.is_current() || !output.allows(&source, &job.message_id) {
        return;
    }
    job.validity.with_current(|| {
        output.translation_started_with_message(
            subtitle_id,
            &target.target_language,
            preferred,
            &job.message_id,
            &source,
        )
    });
    let progress_output = output.clone();
    let progress_message_id = job.message_id.clone();
    let progress_source = source.clone();
    let target_language = target.target_language.clone();
    let progress_validity = job.validity.clone();
    let last_progress = Mutex::new(Instant::now() - Duration::from_millis(80));
    let progress = move |text: &str| {
        let Ok(mut last) = last_progress.lock() else {
            return;
        };
        if last.elapsed() < Duration::from_millis(80) {
            return;
        }
        *last = Instant::now();
        progress_validity.with_current(|| {
            progress_output.translation_partial_with_message(
                subtitle_id,
                text.to_owned(),
                target_language.clone(),
                preferred,
                &progress_message_id,
                &progress_source,
            )
        });
    };
    // Waiting for the shared database lock must not stall an async worker.
    let mut context = tokio::task::spawn_blocking({
        let database = Arc::clone(&database);
        let prompt = job.prompt.clone();
        move || match database.lock() {
            Ok(database) => database
                .recent_translation_context(&prompt, Some(subtitle_id))
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, "translation context could not be loaded");
                    Vec::new()
                }),
            Err(_) => {
                tracing::warn!("translation context database lock is unavailable");
                Vec::new()
            }
        }
    })
    .await
    .unwrap_or_default();
    if job.include_vrcx_context {
        if let Some(entry) = vrcx.translation_context_entry() {
            context.push(entry);
        }
    }
    if !job.validity.is_current() || !output.allows(&source, &job.message_id) {
        return;
    }
    let translation = async {
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
            Err(error) if retry_delay(&error).is_some() => {
                tokio::time::sleep(retry_delay(&error).unwrap()).await;
                if !job.validity.is_current() || !output.allows(&source, &job.message_id) {
                    return None;
                }
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
        Some(result)
    };
    let result = tokio::select! {
        biased;
        _ = job.validity.cancelled() => return,
        result = translation => result,
    };
    let Some(result) = result else { return };
    if !job.validity.is_current() || !output.allows(&source, &job.message_id) {
        return;
    }
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
                let filter = output.microphone_speakers().clone();
                let source = source.clone();
                let message_id = job.message_id.clone();
                let validity = job.validity.clone();
                move || {
                    filter
                        .with_allowed(&source, &message_id, || {
                            let database = database
                                .lock()
                                .map_err(|_| "Database lock is unavailable".to_string())?;
                            validity
                                .with_current(|| {
                                    let catalog_changed = database
                                        .save_translation(subtitle_id, &record)
                                        .map_err(|error| error.to_string())?;
                                    if catalog_changed {
                                        catalog.request();
                                    }
                                    Ok::<_, String>(())
                                })
                                .unwrap_or(Ok(()))
                        })
                        .unwrap_or(Ok(()))
                }
            })
            .await;
            let _current = job.validity.with_current(|| match stored {
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
            });
        }
        Err(error) => {
            job.validity.with_current(|| {
                output.translation_failed_with_message(TranslationFailure {
                    subtitle_id,
                    code: error.code.into(),
                    detail: error.detail,
                    target_language: &target.target_language,
                    preferred,
                    message_id: &job.message_id,
                    source: &source,
                })
            });
        }
    }
}
