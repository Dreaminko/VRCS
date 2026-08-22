use std::collections::{HashMap, HashSet, VecDeque};
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(test)]
use rosc::{OscPacket, OscType};
use serde::Serialize;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot, watch};

use crate::chatbox::NewChatboxMessage;
use crate::config::OscConfig;
use crate::db::Database;
use crate::domain_events::DomainEventHub;
use crate::models::{now_iso8601, Subtitle, SubtitleTranslation};

mod message;
mod transport;

use message::format_chatbox;
#[cfg(test)]
use message::CHATBOX_LIMIT;
use transport::send_chatbox;
#[cfg(test)]
use transport::CHATBOX_ADDRESS;
const TRANSLATION_GRACE: Duration = Duration::from_millis(1_200);
const SEND_INTERVAL: Duration = Duration::from_millis(1_500);
const LATE_TRANSLATION_TTL: Duration = Duration::from_secs(30);
const DISPLAY_QUEUE_CAPACITY: usize = 4;
const EVENT_QUEUE_CAPACITY: usize = 64;
const MANUAL_SEND_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize)]
pub struct OscRuntimeStatus {
    pub enabled: bool,
    pub target: String,
    pub status: String,
    pub last_error: Option<String>,
    pub last_sent_at: Option<String>,
    pub dropped_messages: u64,
    pub send_gate: String,
}

#[derive(Debug, Clone)]
pub struct ManualSendError {
    pub code: &'static str,
    pub detail: String,
}

#[derive(Clone)]
pub struct OscChatboxDispatcher {
    sender: mpsc::Sender<OscEvent>,
    config: watch::Sender<OscConfigState>,
    status: Arc<Mutex<OscRuntimeStatus>>,
}

enum OscEvent {
    Subtitle {
        generation: u64,
        message_id: String,
        subtitle: Subtitle,
        wait_for_translation: bool,
        target_languages: Vec<String>,
    },
    TranslationCompleted {
        generation: u64,
        subtitle_id: i64,
        translation: SubtitleTranslation,
        preferred: bool,
    },
    TranslationFailed {
        generation: u64,
        subtitle_id: i64,
        target_language: String,
    },
    Test {
        automatic_revision: u64,
    },
    Manual {
        config_revision: u64,
        text: String,
        responder: oneshot::Sender<Result<String, ManualSendError>>,
    },
}

impl OscEvent {
    fn is_current(&self, state: &OscConfigState) -> bool {
        match self {
            Self::Subtitle { generation, .. }
            | Self::TranslationCompleted { generation, .. }
            | Self::TranslationFailed { generation, .. } => *generation == state.automatic_revision,
            Self::Test { automatic_revision } => *automatic_revision == state.automatic_revision,
            Self::Manual {
                config_revision, ..
            } => *config_revision == state.config_revision,
        }
    }
}

#[derive(Clone)]
struct OscConfigState {
    config: OscConfig,
    config_revision: u64,
    automatic_revision: u64,
    send_gate: SendGate,
}

#[derive(Clone, Copy, PartialEq)]
enum SendGate {
    Open,
    VrchatMuted,
    MuteUnknown,
}

impl SendGate {
    fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::VrchatMuted => "blocked_vrchat_muted",
            Self::MuteUnknown => "blocked_mute_unknown",
        }
    }

    fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Open => None,
            Self::VrchatMuted => Some("osc.blocked_vrchat_muted"),
            Self::MuteUnknown => Some("osc.blocked_mute_unknown"),
        }
    }
}

struct PendingMessage {
    message_id: String,
    subtitle_id: Option<i64>,
    original: String,
    preferred_target: Option<String>,
    desired_target: Option<String>,
    translations: HashMap<String, String>,
    completed_targets: Vec<String>,
    expected_targets: Vec<String>,
    failed_targets: HashSet<String>,
    rendered_text: Option<String>,
    ready_at: Instant,
    responder: Option<oneshot::Sender<Result<String, ManualSendError>>>,
}

struct SentMessage {
    message_id: String,
    subtitle_id: i64,
    original: String,
    desired_target: Option<String>,
    translation: Option<String>,
    sent_at: Instant,
}

struct AutomaticMessageRecord<'a> {
    db: Option<&'a Arc<Mutex<Database>>>,
    events: &'a DomainEventHub,
    message: &'a PendingMessage,
    rendered_text: &'a str,
    outcome: AutomaticMessageOutcome<'a>,
    formatting: AutomaticMessageFormatting<'a>,
}

struct AutomaticMessageOutcome<'a> {
    status: &'a str,
    error_detail: Option<&'a str>,
    sent_at: Option<String>,
}

struct AutomaticMessageFormatting<'a> {
    preserve_original_text: bool,
    translation_strategy: &'a str,
}

impl OscChatboxDispatcher {
    #[cfg(test)]
    pub fn new(config: OscConfig) -> Self {
        Self::create(config, None, DomainEventHub::new())
    }

    pub fn new_with_db_and_events(
        config: OscConfig,
        db: Arc<Mutex<Database>>,
        events: DomainEventHub,
    ) -> Self {
        Self::create(config, Some(db), events)
    }

    fn create(config: OscConfig, db: Option<Arc<Mutex<Database>>>, events: DomainEventHub) -> Self {
        let (sender, receiver) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let status = Arc::new(Mutex::new(runtime_status(&config)));
        let (config, config_rx) = watch::channel(OscConfigState {
            send_gate: initial_gate(&config),
            config,
            config_revision: 0,
            automatic_revision: 0,
        });
        tokio::spawn(run_worker(
            receiver,
            config_rx,
            Arc::clone(&status),
            db,
            events,
        ));
        Self {
            sender,
            config,
            status,
        }
    }

    #[cfg(test)]
    pub fn publish_subtitle(&self, subtitle: Subtitle, wait_for_translation: bool) {
        self.publish_subtitle_with_targets(
            subtitle,
            wait_for_translation,
            Vec::new(),
            format!("utterance-{}", uuid::Uuid::new_v4()),
        );
    }

    pub fn publish_subtitle_with_targets(
        &self,
        subtitle: Subtitle,
        wait_for_translation: bool,
        target_languages: Vec<String>,
        message_id: String,
    ) {
        let Ok(generation) = self.active_generation() else {
            return;
        };
        if subtitle.source != "microphone" {
            return;
        }
        self.try_send(OscEvent::Subtitle {
            generation,
            message_id,
            subtitle,
            wait_for_translation,
            target_languages,
        });
    }

    pub fn translation_completed(
        &self,
        subtitle_id: i64,
        translation: SubtitleTranslation,
        preferred: bool,
    ) {
        if let Ok(generation) = self.active_generation() {
            self.try_send(OscEvent::TranslationCompleted {
                generation,
                subtitle_id,
                translation,
                preferred,
            });
        }
    }

    pub fn translation_failed(&self, subtitle_id: i64, target_language: &str, _preferred: bool) {
        if let Ok(generation) = self.active_generation() {
            self.try_send(OscEvent::TranslationFailed {
                generation,
                subtitle_id,
                target_language: target_language.into(),
            });
        }
    }

    pub fn queue_test(&self) -> Result<(), &'static str> {
        let automatic_revision = self.active_generation()?;
        self.sender
            .try_send(OscEvent::Test { automatic_revision })
            .map_err(|_| {
                self.record_drop();
                "osc.queue_full"
            })
    }

    pub async fn send_manual(&self, text: String) -> Result<String, ManualSendError> {
        let config_revision = self.active_manual_revision().map_err(manual_gate_error)?;
        let (responder, receiver) = oneshot::channel();
        self.sender
            .try_send(OscEvent::Manual {
                config_revision,
                text,
                responder,
            })
            .map_err(|_| {
                self.record_drop();
                ManualSendError {
                    code: "osc.queue_full",
                    detail: "OSC chatbox queue is full".into(),
                }
            })?;
        tokio::time::timeout(MANUAL_SEND_TIMEOUT, receiver)
            .await
            .map_err(|_| ManualSendError {
                code: "osc.send_timeout",
                detail: "OSC chatbox send timed out".into(),
            })?
            .map_err(|_| ManualSendError {
                code: "osc.send_cancelled",
                detail: "OSC chatbox send was cancelled by a configuration change".into(),
            })?
    }

    pub fn update_config(&self, config: OscConfig) {
        *self.status.lock().expect("OSC status lock") = runtime_status(&config);
        self.config.send_modify(|state| {
            if config.mute_sync_enabled && !state.config.mute_sync_enabled {
                state.send_gate = SendGate::MuteUnknown;
            } else if !config.mute_sync_enabled {
                state.send_gate = SendGate::Open;
            }
            state.config = config;
            state.config_revision = state.config_revision.wrapping_add(1);
            state.automatic_revision = state.automatic_revision.wrapping_add(1);
        });
        self.refresh_gate_status();
    }

    pub fn update_mute_status(&self, muted: Option<bool>) {
        self.config.send_if_modified(|state| {
            let next = if !state.config.mute_sync_enabled {
                SendGate::Open
            } else {
                match muted {
                    Some(true) => SendGate::VrchatMuted,
                    Some(false) => SendGate::Open,
                    None => SendGate::MuteUnknown,
                }
            };
            if next == state.send_gate {
                return false;
            }
            state.send_gate = next;
            state.automatic_revision = state.automatic_revision.wrapping_add(1);
            true
        });
        self.refresh_gate_status();
    }

    pub fn status(&self) -> OscRuntimeStatus {
        self.status.lock().expect("OSC status lock").clone()
    }

    fn active_generation(&self) -> Result<u64, &'static str> {
        let state = self.config.borrow();
        if !state.config.enabled {
            return Err("osc.disabled");
        }
        if let Some(code) = state.send_gate.error_code() {
            return Err(code);
        }
        Ok(state.automatic_revision)
    }

    fn active_manual_revision(&self) -> Result<u64, &'static str> {
        let state = self.config.borrow();
        if !state.config.enabled {
            return Err("osc.disabled");
        }
        Ok(state.config_revision)
    }

    fn refresh_gate_status(&self) {
        let state = self.config.borrow();
        let mut status = self.status.lock().expect("OSC status lock");
        status.send_gate = state.send_gate.label().into();
    }

    fn try_send(&self, event: OscEvent) {
        if self.sender.try_send(event).is_err() {
            self.record_drop();
        }
    }

    fn record_drop(&self) {
        self.status
            .lock()
            .expect("OSC status lock")
            .dropped_messages += 1;
    }
}

struct OscWorkerState {
    config: OscConfigState,
    queue: VecDeque<PendingMessage>,
    latest_subtitle_id: Option<i64>,
    current_sent: Option<SentMessage>,
    last_send: Option<Instant>,
    round_robin_index: usize,
}

impl OscWorkerState {
    fn new(config: OscConfigState) -> Self {
        Self {
            config,
            queue: VecDeque::new(),
            latest_subtitle_id: None,
            current_sent: None,
            last_send: None,
            round_robin_index: 0,
        }
    }
}

enum OscWorkerInput {
    ConfigChanged(OscConfigState),
    SendCancelledByConfig(OscConfigState),
    Event {
        event: OscEvent,
        now: Instant,
        generated_message_id: Option<String>,
    },
    Tick(Instant),
    SendCompleted {
        request: OscSendRequest,
        outcome: OscSendOutcome,
        completed_at: Instant,
    },
}

struct OscSendRequest {
    message: PendingMessage,
    rendered_text: String,
    translation: Option<String>,
    port: u16,
    preserve_original_text: bool,
    translation_strategy: String,
}

impl OscSendRequest {
    fn is_manual(&self) -> bool {
        self.message.responder.is_some()
    }
}

enum OscSendOutcome {
    Sent { sent_at: String },
    Failed(String),
}

enum OscEffect {
    DiscardPending(PendingMessage),
    DiscardResponder(oneshot::Sender<Result<String, ManualSendError>>),
    RejectPending {
        message: PendingMessage,
        code: &'static str,
        detail: &'static str,
    },
    RecordDrop,
    Send(OscSendRequest),
    FinalizeSend {
        request: OscSendRequest,
        outcome: OscSendOutcome,
    },
}

fn reduce_osc_event(state: &mut OscWorkerState, input: OscWorkerInput) -> Vec<OscEffect> {
    match input {
        OscWorkerInput::ConfigChanged(next) => reduce_config_change(state, next),
        OscWorkerInput::SendCancelledByConfig(next) => {
            state.config = next;
            state.current_sent = None;
            state.last_send = None;
            discard_all_pending(&mut state.queue)
        }
        OscWorkerInput::Event {
            event,
            now,
            generated_message_id,
        } => reduce_received_event(state, event, now, generated_message_id),
        OscWorkerInput::Tick(now) => reduce_tick(state, now),
        OscWorkerInput::SendCompleted {
            request,
            outcome,
            completed_at,
        } => reduce_send_completed(state, request, outcome, completed_at),
    }
}

fn reduce_config_change(state: &mut OscWorkerState, next: OscConfigState) -> Vec<OscEffect> {
    let effects = if next.config_revision != state.config.config_revision {
        state.current_sent = None;
        state.latest_subtitle_id = None;
        state.last_send = None;
        discard_all_pending(&mut state.queue)
    } else if next.automatic_revision != state.config.automatic_revision {
        state.current_sent = None;
        state.latest_subtitle_id = None;
        discard_automatic_pending(&mut state.queue)
    } else {
        Vec::new()
    };
    state.config = next;
    effects
}

fn reduce_received_event(
    state: &mut OscWorkerState,
    event: OscEvent,
    now: Instant,
    generated_message_id: Option<String>,
) -> Vec<OscEffect> {
    if !event.is_current(&state.config) {
        return match event {
            OscEvent::Manual { responder, .. } => vec![OscEffect::DiscardResponder(responder)],
            _ => Vec::new(),
        };
    }
    match event {
        OscEvent::Test { .. } => enqueue_pending(
            &mut state.queue,
            PendingMessage {
                message_id: generated_message_id.expect("test messages have generated IDs"),
                subtitle_id: None,
                original: "VRCS OSC test".into(),
                preferred_target: None,
                desired_target: None,
                translations: HashMap::new(),
                completed_targets: Vec::new(),
                expected_targets: Vec::new(),
                failed_targets: HashSet::new(),
                rendered_text: None,
                ready_at: now,
                responder: None,
            },
            false,
        ),
        OscEvent::Manual {
            text, responder, ..
        } => enqueue_pending(
            &mut state.queue,
            PendingMessage {
                message_id: generated_message_id.expect("manual messages have generated IDs"),
                subtitle_id: None,
                original: String::new(),
                preferred_target: None,
                desired_target: None,
                translations: HashMap::new(),
                completed_targets: Vec::new(),
                expected_targets: Vec::new(),
                failed_targets: HashSet::new(),
                rendered_text: Some(text),
                ready_at: now,
                responder: Some(responder),
            },
            true,
        ),
        OscEvent::Subtitle {
            message_id,
            subtitle,
            wait_for_translation,
            target_languages,
            ..
        } => {
            let Some(subtitle_id) = subtitle.id else {
                return Vec::new();
            };
            state.latest_subtitle_id = Some(subtitle_id);
            state.current_sent = None;
            let preferred_target = target_languages.first().cloned();
            let desired_target = select_translation_target(
                &state.config.config.translation_strategy,
                &target_languages,
                &mut state.round_robin_index,
            );
            enqueue_pending(
                &mut state.queue,
                PendingMessage {
                    message_id,
                    subtitle_id: Some(subtitle_id),
                    original: subtitle.text,
                    preferred_target,
                    desired_target,
                    translations: HashMap::new(),
                    completed_targets: Vec::new(),
                    expected_targets: target_languages,
                    failed_targets: HashSet::new(),
                    rendered_text: None,
                    ready_at: now
                        + if wait_for_translation {
                            TRANSLATION_GRACE
                        } else {
                            Duration::ZERO
                        },
                    responder: None,
                },
                false,
            )
        }
        OscEvent::TranslationFailed {
            subtitle_id,
            target_language,
            ..
        } => {
            if let Some(message) = state
                .queue
                .iter_mut()
                .find(|item| item.subtitle_id == Some(subtitle_id))
            {
                message.failed_targets.insert(target_language);
                let all_languages = state.config.config.translation_strategy == "all_languages";
                let all_failed = !message.expected_targets.is_empty()
                    && message.failed_targets.len() >= message.expected_targets.len();
                if (all_languages && all_targets_resolved(message))
                    || (!all_languages && (all_failed || selected_translation(message).is_some()))
                {
                    message.ready_at = now;
                }
            }
            Vec::new()
        }
        OscEvent::TranslationCompleted {
            subtitle_id,
            translation,
            preferred,
            ..
        } => reduce_translation_completed(state, subtitle_id, translation, preferred, now),
    }
}

fn reduce_translation_completed(
    state: &mut OscWorkerState,
    subtitle_id: i64,
    translation: SubtitleTranslation,
    preferred: bool,
    now: Instant,
) -> Vec<OscEffect> {
    if let Some(message) = state
        .queue
        .iter_mut()
        .find(|item| item.subtitle_id == Some(subtitle_id))
    {
        let language = translation.target_language.clone();
        if !message.translations.contains_key(&language) {
            message.completed_targets.push(language.clone());
        }
        message
            .translations
            .insert(language.clone(), translation.text);
        let all_languages = state.config.config.translation_strategy == "all_languages";
        if (all_languages && all_targets_resolved(message))
            || (!all_languages
                && (message.desired_target.as_deref() == Some(&language)
                    || (preferred && message.desired_target.is_none())
                    || message
                        .desired_target
                        .as_ref()
                        .is_some_and(|target| message.failed_targets.contains(target))))
        {
            message.ready_at = now;
        }
        return Vec::new();
    }
    if state.latest_subtitle_id != Some(subtitle_id) {
        return Vec::new();
    }
    let Some(sent) = state.current_sent.as_ref().filter(|sent| {
        sent.subtitle_id == subtitle_id
            && sent.translation.is_none()
            && sent
                .desired_target
                .as_deref()
                .is_none_or(|target| target == translation.target_language)
            && now.saturating_duration_since(sent.sent_at) <= LATE_TRANSLATION_TTL
    }) else {
        return Vec::new();
    };
    enqueue_pending(
        &mut state.queue,
        PendingMessage {
            message_id: sent.message_id.clone(),
            subtitle_id: Some(subtitle_id),
            original: sent.original.clone(),
            preferred_target: Some(translation.target_language.clone()),
            desired_target: Some(translation.target_language.clone()),
            translations: HashMap::from([(translation.target_language.clone(), translation.text)]),
            completed_targets: vec![translation.target_language],
            expected_targets: Vec::new(),
            failed_targets: HashSet::new(),
            rendered_text: None,
            ready_at: now,
            responder: None,
        },
        false,
    )
}

fn reduce_tick(state: &mut OscWorkerState, now: Instant) -> Vec<OscEffect> {
    if !state.config.config.enabled {
        return discard_all_pending(&mut state.queue);
    }
    let mut effects = if state.config.send_gate != SendGate::Open {
        discard_automatic_pending(&mut state.queue)
    } else {
        Vec::new()
    };
    let ready = state
        .queue
        .front()
        .is_some_and(|message| message.ready_at <= now);
    let rate_ready = state
        .last_send
        .is_none_or(|sent| now.saturating_duration_since(sent) >= SEND_INTERVAL);
    if !ready || !rate_ready {
        return effects;
    }
    let Some(message) = state.queue.pop_front() else {
        return effects;
    };
    let translation =
        selected_translation_text(&message, &state.config.config.translation_strategy);
    let rendered_text = message.rendered_text.clone().unwrap_or_else(|| {
        format_chatbox(
            &message.original,
            translation.as_deref(),
            state.config.config.preserve_original_text,
        )
    });
    effects.push(OscEffect::Send(OscSendRequest {
        message,
        rendered_text,
        translation,
        port: state.config.config.port,
        preserve_original_text: state.config.config.preserve_original_text,
        translation_strategy: state.config.config.translation_strategy.clone(),
    }));
    effects
}

fn reduce_send_completed(
    state: &mut OscWorkerState,
    request: OscSendRequest,
    outcome: OscSendOutcome,
    completed_at: Instant,
) -> Vec<OscEffect> {
    if matches!(outcome, OscSendOutcome::Sent { .. }) {
        state.last_send = Some(completed_at);
        if let Some(subtitle_id) = request.message.subtitle_id {
            state.current_sent = Some(SentMessage {
                message_id: request.message.message_id.clone(),
                subtitle_id,
                original: request.message.original.clone(),
                desired_target: request.message.desired_target.clone(),
                translation: request.translation.clone(),
                sent_at: completed_at,
            });
        }
    }
    vec![OscEffect::FinalizeSend { request, outcome }]
}

fn enqueue_pending(
    queue: &mut VecDeque<PendingMessage>,
    message: PendingMessage,
    priority: bool,
) -> Vec<OscEffect> {
    let mut effects = Vec::new();
    if queue.len() == DISPLAY_QUEUE_CAPACITY {
        let automatic = queue.iter().position(|queued| queued.responder.is_none());
        let dropped = if let Some(index) = automatic {
            queue.remove(index)
        } else if priority {
            queue.pop_front()
        } else {
            effects.push(OscEffect::RejectPending {
                message,
                code: "osc.queue_full",
                detail: "OSC chatbox queue is full",
            });
            effects.push(OscEffect::RecordDrop);
            return effects;
        };
        if let Some(message) = dropped {
            effects.push(OscEffect::RejectPending {
                message,
                code: "osc.queue_full",
                detail: "OSC chatbox queue is full",
            });
        }
        effects.push(OscEffect::RecordDrop);
    }
    if priority {
        let index = queue
            .iter()
            .position(|queued| queued.responder.is_none())
            .unwrap_or(queue.len());
        queue.insert(index, message);
    } else {
        queue.push_back(message);
    }
    effects
}

fn discard_all_pending(queue: &mut VecDeque<PendingMessage>) -> Vec<OscEffect> {
    queue.drain(..).map(OscEffect::DiscardPending).collect()
}

fn discard_automatic_pending(queue: &mut VecDeque<PendingMessage>) -> Vec<OscEffect> {
    let mut retained = VecDeque::with_capacity(queue.len());
    let mut effects = Vec::new();
    while let Some(message) = queue.pop_front() {
        if message.responder.is_some() {
            retained.push_back(message);
        } else {
            effects.push(OscEffect::DiscardPending(message));
        }
    }
    *queue = retained;
    effects
}

enum SendExecution {
    Completed(OscSendOutcome, Instant),
    ConfigChanged(OscConfigState),
    ConfigClosed,
}

async fn execute_send(
    socket: &Result<UdpSocket, std::io::Error>,
    config_rx: &mut watch::Receiver<OscConfigState>,
    request: &OscSendRequest,
) -> SendExecution {
    let result = match socket {
        Ok(socket) if request.is_manual() => {
            send_chatbox(socket, request.port, &request.rendered_text).await
        }
        Ok(socket) => {
            tokio::select! {
                biased;
                changed = config_rx.changed() => {
                    return if changed.is_err() {
                        SendExecution::ConfigClosed
                    } else {
                        SendExecution::ConfigChanged(config_rx.borrow().clone())
                    };
                }
                result = send_chatbox(socket, request.port, &request.rendered_text) => result,
            }
        }
        Err(error) => Err(error.to_string()),
    };
    let completed_at = Instant::now();
    let outcome = match result {
        Ok(()) => OscSendOutcome::Sent {
            sent_at: now_iso8601(),
        },
        Err(error) => OscSendOutcome::Failed(error),
    };
    SendExecution::Completed(outcome, completed_at)
}

fn execute_osc_effect(
    effect: OscEffect,
    status: &Arc<Mutex<OscRuntimeStatus>>,
    db: Option<&Arc<Mutex<Database>>>,
    events: &DomainEventHub,
) {
    match effect {
        OscEffect::DiscardPending(message) => drop(message),
        OscEffect::DiscardResponder(responder) => drop(responder),
        OscEffect::RejectPending {
            message,
            code,
            detail,
        } => fail_pending(message, code, detail),
        OscEffect::RecordDrop => {
            status.lock().expect("OSC status lock").dropped_messages += 1;
        }
        OscEffect::Send(_) => unreachable!("send effects are executed by the worker"),
        OscEffect::FinalizeSend {
            mut request,
            outcome,
        } => {
            let mut runtime = status.lock().expect("OSC status lock");
            match outcome {
                OscSendOutcome::Sent { sent_at } => {
                    runtime.status = "ready".into();
                    runtime.last_error = None;
                    runtime.last_sent_at = Some(sent_at.clone());
                    record_automatic_message(AutomaticMessageRecord {
                        db,
                        events,
                        message: &request.message,
                        rendered_text: &request.rendered_text,
                        outcome: AutomaticMessageOutcome {
                            status: "sent",
                            error_detail: None,
                            sent_at: Some(sent_at.clone()),
                        },
                        formatting: AutomaticMessageFormatting {
                            preserve_original_text: request.preserve_original_text,
                            translation_strategy: &request.translation_strategy,
                        },
                    });
                    if let Some(responder) = request.message.responder.take() {
                        let _ = responder.send(Ok(sent_at));
                    }
                }
                OscSendOutcome::Failed(error) => {
                    runtime.status = "error".into();
                    runtime.last_error = Some(error.clone());
                    record_automatic_message(AutomaticMessageRecord {
                        db,
                        events,
                        message: &request.message,
                        rendered_text: &request.rendered_text,
                        outcome: AutomaticMessageOutcome {
                            status: "failed",
                            error_detail: Some(&error),
                            sent_at: None,
                        },
                        formatting: AutomaticMessageFormatting {
                            preserve_original_text: request.preserve_original_text,
                            translation_strategy: &request.translation_strategy,
                        },
                    });
                    if let Some(responder) = request.message.responder.take() {
                        let _ = responder.send(Err(ManualSendError {
                            code: "osc.send_failed",
                            detail: error,
                        }));
                    }
                }
            }
        }
    }
}

async fn run_worker(
    mut receiver: mpsc::Receiver<OscEvent>,
    mut config_rx: watch::Receiver<OscConfigState>,
    status: Arc<Mutex<OscRuntimeStatus>>,
    db: Option<Arc<Mutex<Database>>>,
    events: DomainEventHub,
) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await;
    let mut state = OscWorkerState::new(config_rx.borrow().clone());
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    'worker: loop {
        let input = tokio::select! {
            biased;
            changed = config_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                OscWorkerInput::ConfigChanged(config_rx.borrow().clone())
            }
            event = receiver.recv() => {
                let Some(event) = event else { break };
                let generated_message_id = matches!(event, OscEvent::Test { .. } | OscEvent::Manual { .. })
                    .then(|| format!("chatbox-{}", uuid::Uuid::new_v4()));
                OscWorkerInput::Event {
                    event,
                    now: Instant::now(),
                    generated_message_id,
                }
            }
            _ = tick.tick() => OscWorkerInput::Tick(Instant::now()),
        };
        let mut effects = VecDeque::from(reduce_osc_event(&mut state, input));
        while let Some(effect) = effects.pop_front() {
            let OscEffect::Send(request) = effect else {
                execute_osc_effect(effect, &status, db.as_ref(), &events);
                continue;
            };
            match execute_send(&socket, &mut config_rx, &request).await {
                SendExecution::Completed(outcome, completed_at) => {
                    effects.extend(reduce_osc_event(
                        &mut state,
                        OscWorkerInput::SendCompleted {
                            request,
                            outcome,
                            completed_at,
                        },
                    ))
                }
                SendExecution::ConfigChanged(next) => effects.extend(reduce_osc_event(
                    &mut state,
                    OscWorkerInput::SendCancelledByConfig(next),
                )),
                SendExecution::ConfigClosed => break 'worker,
            }
        }
    }
}

fn record_automatic_message(record: AutomaticMessageRecord<'_>) {
    let AutomaticMessageRecord {
        db,
        events,
        message,
        rendered_text,
        outcome,
        formatting,
    } = record;
    if message.subtitle_id.is_none() || message.rendered_text.is_some() {
        return;
    }
    let Some(db) = db else { return };
    let original = crate::chatbox::compact_text(&message.original);
    let translation = selected_translation_text(message, formatting.translation_strategy);
    let effective_translation = translation
        .as_deref()
        .filter(|value| !value.is_empty() && *value != original.as_str());
    let (send_mode, untruncated) = match (effective_translation, formatting.preserve_original_text)
    {
        (Some(value), true) => ("bilingual", format!("{original}\n{value}")),
        (Some(value), false) => ("translation", value.to_owned()),
        (None, _) => ("original", original),
    };
    let record = NewChatboxMessage {
        source: "microphone".into(),
        original: message.original.clone(),
        translation,
        source_language: None,
        target_language: selected_target_language(message, formatting.translation_strategy),
        send_mode: send_mode.into(),
        message_format: "original_newline_translation".into(),
        custom_format: None,
        rendered_text: rendered_text.into(),
        char_count: rendered_text.chars().count(),
        truncated: rendered_text != untruncated,
        status: outcome.status.into(),
        error_code: outcome.error_detail.map(|_| "osc.send_failed".into()),
        error_detail: outcome.error_detail.map(str::to_owned),
        resent_from_id: None,
        created_at: now_iso8601(),
        sent_at: outcome.sent_at,
    };
    if let Ok(database) = db.lock() {
        match database.add_chatbox_message(&record) {
            Ok(saved) if outcome.status == "sent" => {
                events.chatbox_sent(&message.message_id, &saved)
            }
            Ok(_) => {}
            Err(error) => tracing::warn!("Failed to store automatic Chatbox history: {error}"),
        }
    }
}

fn selected_translation(message: &PendingMessage) -> Option<&String> {
    message
        .desired_target
        .as_ref()
        .and_then(|target| message.translations.get(target))
        .or_else(|| {
            message
                .preferred_target
                .as_ref()
                .and_then(|target| message.translations.get(target))
        })
        .or_else(|| {
            message
                .completed_targets
                .iter()
                .find_map(|target| message.translations.get(target))
        })
}

fn selected_translation_text(message: &PendingMessage, strategy: &str) -> Option<String> {
    if strategy != "all_languages" {
        return selected_translation(message).cloned();
    }
    let original = crate::chatbox::compact_text(&message.original);
    let mut translations = Vec::new();
    for target in &message.expected_targets {
        let Some(value) = message.translations.get(target) else {
            continue;
        };
        let value = crate::chatbox::compact_text(value);
        if value.is_empty() || value == original || translations.contains(&value) {
            continue;
        }
        translations.push(value);
    }
    (!translations.is_empty()).then(|| translations.join(" / "))
}

fn selected_target_language(message: &PendingMessage, strategy: &str) -> Option<String> {
    if strategy != "all_languages" {
        return message
            .desired_target
            .clone()
            .or_else(|| message.preferred_target.clone());
    }
    let targets = message
        .expected_targets
        .iter()
        .filter(|target| message.translations.contains_key(*target))
        .cloned()
        .collect::<Vec<_>>();
    (!targets.is_empty()).then(|| targets.join(","))
}

fn all_targets_resolved(message: &PendingMessage) -> bool {
    !message.expected_targets.is_empty()
        && message.expected_targets.iter().all(|target| {
            message.translations.contains_key(target) || message.failed_targets.contains(target)
        })
}

fn select_translation_target(
    strategy: &str,
    target_languages: &[String],
    round_robin_index: &mut usize,
) -> Option<String> {
    if strategy == "all_languages" {
        return None;
    }
    if strategy != "round_robin" || target_languages.is_empty() {
        return target_languages.first().cloned();
    }
    let selected = target_languages[*round_robin_index % target_languages.len()].clone();
    *round_robin_index = round_robin_index.wrapping_add(1);
    Some(selected)
}

fn fail_pending(message: PendingMessage, code: &'static str, detail: &'static str) {
    if let Some(responder) = message.responder {
        let _ = responder.send(Err(ManualSendError {
            code,
            detail: detail.into(),
        }));
    }
}

fn manual_gate_error(code: &'static str) -> ManualSendError {
    let detail = match code {
        "osc.disabled" => "OSC chatbox output is disabled",
        "osc.blocked_vrchat_muted" => "OSC chatbox output is blocked because VRChat is muted",
        "osc.blocked_mute_unknown" => {
            "OSC chatbox output is blocked until the VRChat mute state is known"
        }
        _ => "OSC chatbox output is unavailable",
    };
    ManualSendError {
        code,
        detail: detail.into(),
    }
}

fn runtime_status(config: &OscConfig) -> OscRuntimeStatus {
    OscRuntimeStatus {
        enabled: config.enabled,
        target: format!("127.0.0.1:{}", config.port),
        status: if config.enabled { "ready" } else { "disabled" }.into(),
        last_error: None,
        last_sent_at: None,
        dropped_messages: 0,
        send_gate: initial_gate(config).label().into(),
    }
}

fn initial_gate(config: &OscConfig) -> SendGate {
    if config.mute_sync_enabled {
        SendGate::MuteUnknown
    } else {
        SendGate::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_state() -> OscConfigState {
        OscConfigState {
            config: OscConfig {
                enabled: true,
                port: 9000,
                mute_sync_enabled: false,
                mute_status_toast_enabled: false,
                preserve_original_text: true,
                translation_strategy: "preferred_only".into(),
            },
            config_revision: 0,
            automatic_revision: 0,
            send_gate: SendGate::Open,
        }
    }

    fn pending_message(targets: &[&str]) -> PendingMessage {
        PendingMessage {
            message_id: "message-test".into(),
            subtitle_id: Some(1),
            original: "こんにちは".into(),
            preferred_target: targets.first().map(|target| (*target).into()),
            desired_target: None,
            translations: HashMap::new(),
            completed_targets: Vec::new(),
            expected_targets: targets.iter().map(|target| (*target).into()).collect(),
            failed_targets: HashSet::new(),
            rendered_text: None,
            ready_at: Instant::now(),
            responder: None,
        }
    }

    fn manual_message(text: &str, ready_at: Instant) -> PendingMessage {
        let (responder, _receiver) = oneshot::channel();
        PendingMessage {
            message_id: format!("manual-{text}"),
            subtitle_id: None,
            original: String::new(),
            preferred_target: None,
            desired_target: None,
            translations: HashMap::new(),
            completed_targets: Vec::new(),
            expected_targets: Vec::new(),
            failed_targets: HashSet::new(),
            rendered_text: Some(text.into()),
            ready_at,
            responder: Some(responder),
        }
    }

    #[test]
    fn config_revision_discards_all_worker_state() {
        let now = Instant::now();
        let mut state = OscWorkerState::new(config_state());
        state.queue.push_back(pending_message(&[]));
        state.queue.push_back(manual_message("manual", now));
        state.latest_subtitle_id = Some(1);
        state.current_sent = Some(SentMessage {
            message_id: "sent".into(),
            subtitle_id: 1,
            original: "original".into(),
            desired_target: None,
            translation: None,
            sent_at: now,
        });
        state.last_send = Some(now);
        let mut next = config_state();
        next.config_revision = 1;
        next.automatic_revision = 1;

        let effects = reduce_osc_event(&mut state, OscWorkerInput::ConfigChanged(next));

        assert_eq!(effects.len(), 2);
        assert!(effects
            .iter()
            .all(|effect| matches!(effect, OscEffect::DiscardPending(_))));
        assert!(state.queue.is_empty());
        assert!(state.current_sent.is_none());
        assert!(state.latest_subtitle_id.is_none());
        assert!(state.last_send.is_none());
    }

    #[test]
    fn automatic_revision_preserves_manual_messages() {
        let now = Instant::now();
        let mut state = OscWorkerState::new(config_state());
        state.queue.push_back(pending_message(&[]));
        state.queue.push_back(manual_message("manual", now));
        let mut next = config_state();
        next.automatic_revision = 1;

        let effects = reduce_osc_event(&mut state, OscWorkerInput::ConfigChanged(next));

        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], OscEffect::DiscardPending(_)));
        assert_eq!(state.queue.len(), 1);
        assert!(state.queue.front().unwrap().responder.is_some());
    }

    #[test]
    fn stale_events_are_ignored_before_message_construction() {
        let mut state = OscWorkerState::new(config_state());
        let effects = reduce_osc_event(
            &mut state,
            OscWorkerInput::Event {
                event: OscEvent::Test {
                    automatic_revision: 1,
                },
                now: Instant::now(),
                generated_message_id: None,
            },
        );
        assert!(effects.is_empty());
        assert!(state.queue.is_empty());
    }

    #[test]
    fn bounded_queue_drops_automatic_messages_for_manual_priority() {
        let now = Instant::now();
        let mut queue = VecDeque::new();
        for index in 0..DISPLAY_QUEUE_CAPACITY {
            let mut message = pending_message(&[]);
            message.message_id = format!("automatic-{index}");
            queue.push_back(message);
        }

        let effects = enqueue_pending(&mut queue, manual_message("priority", now), true);

        assert_eq!(queue.len(), DISPLAY_QUEUE_CAPACITY);
        assert!(queue.front().unwrap().responder.is_some());
        assert!(matches!(
            effects.as_slice(),
            [OscEffect::RejectPending { .. }, OscEffect::RecordDrop]
        ));
    }

    #[test]
    fn translation_results_make_waiting_messages_ready() {
        let now = Instant::now();
        let mut state = OscWorkerState::new(config_state());
        let mut completed = pending_message(&["en"]);
        completed.desired_target = Some("en".into());
        completed.ready_at = now + TRANSLATION_GRACE;
        state.queue.push_back(completed);

        reduce_osc_event(
            &mut state,
            OscWorkerInput::Event {
                event: OscEvent::TranslationCompleted {
                    generation: 0,
                    subtitle_id: 1,
                    translation: SubtitleTranslation {
                        text: "Hello".into(),
                        source_language: Some("ja".into()),
                        target_language: "en".into(),
                        provider: "local".into(),
                        model: None,
                        created_at: now_iso8601(),
                    },
                    preferred: true,
                },
                now,
                generated_message_id: None,
            },
        );
        assert_eq!(state.queue.front().unwrap().ready_at, now);

        let mut failed = pending_message(&["en"]);
        failed.ready_at = now + TRANSLATION_GRACE;
        state.queue.clear();
        state.queue.push_back(failed);
        reduce_osc_event(
            &mut state,
            OscWorkerInput::Event {
                event: OscEvent::TranslationFailed {
                    generation: 0,
                    subtitle_id: 1,
                    target_language: "en".into(),
                },
                now,
                generated_message_id: None,
            },
        );
        assert_eq!(state.queue.front().unwrap().ready_at, now);
    }

    #[test]
    fn late_translation_resends_only_within_ttl() {
        let now = Instant::now();
        let mut state = OscWorkerState::new(config_state());
        state.latest_subtitle_id = Some(1);
        state.current_sent = Some(SentMessage {
            message_id: "utterance-1".into(),
            subtitle_id: 1,
            original: "こんにちは".into(),
            desired_target: Some("en".into()),
            translation: None,
            sent_at: now - LATE_TRANSLATION_TTL,
        });
        let translation = || SubtitleTranslation {
            text: "Hello".into(),
            source_language: Some("ja".into()),
            target_language: "en".into(),
            provider: "local".into(),
            model: None,
            created_at: now_iso8601(),
        };

        reduce_translation_completed(&mut state, 1, translation(), true, now);
        assert_eq!(state.queue.len(), 1);
        state.queue.clear();
        reduce_translation_completed(
            &mut state,
            1,
            translation(),
            true,
            now + Duration::from_millis(1),
        );
        assert!(state.queue.is_empty());
    }

    #[test]
    fn tick_applies_gate_and_rate_limit_before_sending() {
        let now = Instant::now();
        let mut config = config_state();
        config.send_gate = SendGate::VrchatMuted;
        let mut state = OscWorkerState::new(config);
        state.queue.push_back(pending_message(&[]));
        state.queue.push_back(manual_message("manual", now));

        let effects = reduce_osc_event(&mut state, OscWorkerInput::Tick(now));
        assert!(matches!(
            effects.as_slice(),
            [OscEffect::DiscardPending(_), OscEffect::Send(request)] if request.is_manual()
        ));

        state.config.send_gate = SendGate::Open;
        state.queue.push_back(pending_message(&[]));
        state.last_send = Some(now);
        let effects = reduce_osc_event(
            &mut state,
            OscWorkerInput::Tick(now + SEND_INTERVAL - Duration::from_millis(1)),
        );
        assert!(effects.is_empty());
        assert_eq!(state.queue.len(), 1);
    }

    #[test]
    fn send_completion_updates_state_only_after_success() {
        let now = Instant::now();
        let request = |message_id: &str| OscSendRequest {
            message: PendingMessage {
                message_id: message_id.into(),
                subtitle_id: Some(1),
                original: "こんにちは".into(),
                preferred_target: Some("en".into()),
                desired_target: Some("en".into()),
                translations: HashMap::from([("en".into(), "Hello".into())]),
                completed_targets: vec!["en".into()],
                expected_targets: vec!["en".into()],
                failed_targets: HashSet::new(),
                rendered_text: None,
                ready_at: now,
                responder: None,
            },
            rendered_text: "こんにちは\nHello".into(),
            translation: Some("Hello".into()),
            port: 9000,
            preserve_original_text: true,
            translation_strategy: "preferred_only".into(),
        };
        let mut state = OscWorkerState::new(config_state());

        let effects = reduce_osc_event(
            &mut state,
            OscWorkerInput::SendCompleted {
                request: request("success"),
                outcome: OscSendOutcome::Sent {
                    sent_at: "2025-01-01T00:00:00Z".into(),
                },
                completed_at: now,
            },
        );
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], OscEffect::FinalizeSend { .. }));
        assert_eq!(state.last_send, Some(now));
        assert_eq!(
            state
                .current_sent
                .as_ref()
                .map(|sent| sent.message_id.as_str()),
            Some("success")
        );

        let previous_send = state.last_send;
        let previous_message = state.current_sent.as_ref().unwrap().message_id.clone();
        reduce_osc_event(
            &mut state,
            OscWorkerInput::SendCompleted {
                request: request("failure"),
                outcome: OscSendOutcome::Failed("network".into()),
                completed_at: now + Duration::from_secs(1),
            },
        );
        assert_eq!(state.last_send, previous_send);
        assert_eq!(
            state.current_sent.as_ref().unwrap().message_id,
            previous_message
        );
    }

    #[test]
    fn round_robin_selects_targets_in_route_order() {
        let targets = vec!["en".into(), "ja".into(), "fr".into()];
        let mut index = 0;
        assert_eq!(
            select_translation_target("round_robin", &targets, &mut index).as_deref(),
            Some("en")
        );
        assert_eq!(
            select_translation_target("round_robin", &targets, &mut index).as_deref(),
            Some("ja")
        );
        assert_eq!(
            select_translation_target("round_robin", &targets, &mut index).as_deref(),
            Some("fr")
        );
        assert_eq!(
            select_translation_target("round_robin", &targets, &mut index).as_deref(),
            Some("en")
        );
    }

    #[test]
    fn all_languages_does_not_select_a_single_target() {
        let targets = vec!["en".into(), "ja".into(), "fr".into()];
        let mut index = 2;
        assert_eq!(
            select_translation_target("all_languages", &targets, &mut index),
            None
        );
        assert_eq!(index, 2);
    }

    #[test]
    fn all_languages_combines_translations_in_target_order() {
        let mut message = pending_message(&["zh-Hans", "en", "fr"]);
        message.translations.insert("en".into(), "Hello".into());
        message.translations.insert("zh-Hans".into(), "你好".into());
        message.translations.insert("fr".into(), "Hello".into());

        assert_eq!(
            selected_translation_text(&message, "all_languages").as_deref(),
            Some("你好 / Hello")
        );
        assert_eq!(
            selected_target_language(&message, "all_languages").as_deref(),
            Some("zh-Hans,en,fr")
        );
    }

    #[test]
    fn all_languages_is_resolved_after_each_target_completes_or_fails() {
        let mut message = pending_message(&["zh-Hans", "en", "fr"]);
        message.translations.insert("zh-Hans".into(), "你好".into());
        message.failed_targets.insert("en".into());
        assert!(!all_targets_resolved(&message));

        message.translations.insert("fr".into(), "Bonjour".into());
        assert!(all_targets_resolved(&message));
    }

    fn subtitle(id: i64, source: &str, text: &str) -> Subtitle {
        Subtitle {
            id: Some(id),
            conversation_id: Some("conversation-test".into()),
            text: text.into(),
            language: Some("ja".into()),
            started_at: None,
            ended_at: None,
            source: source.into(),
            created_at: now_iso8601(),
            translations: Vec::new(),
        }
    }

    #[test]
    fn formats_bilingual_messages_within_vrchat_limit() {
        let formatted = format_chatbox(&"原".repeat(100), Some(&"訳".repeat(100)), true);
        let mut lines = formatted.lines();
        assert_eq!(lines.next().unwrap().chars().count(), 71);
        assert_eq!(lines.next().unwrap().chars().count(), 72);
        assert_eq!(formatted.chars().count(), CHATBOX_LIMIT);
    }

    #[test]
    fn removes_control_characters_and_avoids_duplicate_translation() {
        assert_eq!(
            format_chatbox(" hello\0\nworld ", Some("hello world"), true),
            "hello world"
        );
    }

    #[test]
    fn omits_original_text_when_disabled_and_translation_is_available() {
        assert_eq!(format_chatbox("こんにちは", Some("你好"), false), "你好");
        assert_eq!(format_chatbox("こんにちは", None, false), "こんにちは");
    }

    #[tokio::test]
    async fn sends_utf8_chatbox_packet_to_configured_udp_port() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = receiver.local_addr().unwrap().port();
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        send_chatbox(&sender, port, "测试").await.unwrap();

        let mut buffer = [0u8; 512];
        let (length, _) = receiver.recv_from(&mut buffer).await.unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buffer[..length]).unwrap();
        let OscPacket::Message(message) = packet else {
            panic!("expected OSC message")
        };
        assert_eq!(message.addr, CHATBOX_ADDRESS);
        assert_eq!(message.args[0], OscType::String("测试".into()));
        assert_eq!(message.args[1], OscType::Bool(true));
        assert_eq!(message.args[2], OscType::Bool(false));
    }

    #[tokio::test]
    async fn manual_send_ignores_vrchat_mute_state() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let dispatcher = OscChatboxDispatcher::new(OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: true,
            mute_status_toast_enabled: false,
            preserve_original_text: true,
            translation_strategy: "preferred_only".into(),
        });
        dispatcher.update_mute_status(Some(true));
        let send = tokio::spawn(async move { dispatcher.send_manual("手动消息".into()).await });

        let mut buffer = [0u8; 512];
        let (length, _) =
            tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buffer))
                .await
                .expect("manual Chatbox send should ignore VRChat mute state")
                .unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buffer[..length]).unwrap();
        let OscPacket::Message(message) = packet else {
            panic!("expected OSC message")
        };
        assert_eq!(message.args[0], OscType::String("手动消息".into()));
        assert!(send.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn dispatcher_ignores_speaker_and_merges_fast_translation() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let dispatcher = OscChatboxDispatcher::new(OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: false,
            mute_status_toast_enabled: false,
            preserve_original_text: true,
            translation_strategy: "preferred_only".into(),
        });
        dispatcher.publish_subtitle(subtitle(1, "speaker", "ignored"), false);
        assert!(tokio::time::timeout(Duration::from_millis(150), async {
            let mut buffer = [0u8; 512];
            receiver.recv_from(&mut buffer).await
        })
        .await
        .is_err());

        dispatcher.publish_subtitle(subtitle(2, "microphone", "こんにちは"), true);
        dispatcher.translation_completed(
            2,
            SubtitleTranslation {
                text: "你好".into(),
                source_language: Some("ja".into()),
                target_language: "zh-Hans".into(),
                provider: "local".into(),
                model: None,
                created_at: now_iso8601(),
            },
            true,
        );

        let mut buffer = [0u8; 512];
        let (length, _) =
            tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buffer))
                .await
                .unwrap()
                .unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buffer[..length]).unwrap();
        let OscPacket::Message(message) = packet else {
            panic!("expected OSC message")
        };
        assert_eq!(message.args[0], OscType::String("こんにちは\n你好".into()));
    }

    #[tokio::test]
    async fn dispatcher_can_send_translation_without_original_text() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let dispatcher = OscChatboxDispatcher::new(OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: false,
            mute_status_toast_enabled: false,
            preserve_original_text: false,
            translation_strategy: "preferred_only".into(),
        });
        dispatcher.publish_subtitle(subtitle(3, "microphone", "こんにちは"), true);
        dispatcher.translation_completed(
            3,
            SubtitleTranslation {
                text: "你好".into(),
                source_language: Some("ja".into()),
                target_language: "zh-Hans".into(),
                provider: "local".into(),
                model: None,
                created_at: now_iso8601(),
            },
            true,
        );

        let mut buffer = [0u8; 512];
        let (length, _) =
            tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buffer))
                .await
                .unwrap()
                .unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buffer[..length]).unwrap();
        let OscPacket::Message(message) = packet else {
            panic!("expected OSC message")
        };
        assert_eq!(message.args[0], OscType::String("你好".into()));
    }

    #[tokio::test]
    async fn dispatcher_combines_all_languages_in_one_chatbox_message() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let dispatcher = OscChatboxDispatcher::new(OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: false,
            mute_status_toast_enabled: false,
            preserve_original_text: true,
            translation_strategy: "all_languages".into(),
        });
        dispatcher.publish_subtitle_with_targets(
            subtitle(4, "microphone", "こんにちは"),
            true,
            vec!["zh-Hans".into(), "en".into()],
            "utterance-all-languages".into(),
        );
        for (target_language, text) in [("en", "Hello"), ("zh-Hans", "你好")] {
            dispatcher.translation_completed(
                4,
                SubtitleTranslation {
                    text: text.into(),
                    source_language: Some("ja".into()),
                    target_language: target_language.into(),
                    provider: "local".into(),
                    model: None,
                    created_at: now_iso8601(),
                },
                target_language == "zh-Hans",
            );
        }

        let mut buffer = [0u8; 512];
        let (length, _) =
            tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buffer))
                .await
                .unwrap()
                .unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buffer[..length]).unwrap();
        let OscPacket::Message(message) = packet else {
            panic!("expected OSC message")
        };
        assert_eq!(
            message.args[0],
            OscType::String("こんにちは\n你好 / Hello".into())
        );
    }

    #[tokio::test]
    async fn config_change_discards_events_from_a_saturated_previous_generation() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let config = OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: false,
            mute_status_toast_enabled: false,
            preserve_original_text: true,
            translation_strategy: "preferred_only".into(),
        };
        let dispatcher = OscChatboxDispatcher::new(config.clone());

        // A current-thread Tokio test does not run the worker until this task yields,
        // so this fills the entire data channel before the configuration update.
        for id in 0..EVENT_QUEUE_CAPACITY as i64 {
            dispatcher.publish_subtitle(subtitle(id, "microphone", "stale"), false);
        }
        dispatcher.update_config(config);

        let received = tokio::time::timeout(Duration::from_millis(1_700), async {
            let mut buffer = [0u8; 512];
            receiver.recv_from(&mut buffer).await
        })
        .await;
        assert!(
            received.is_err(),
            "old-generation messages must be discarded"
        );
    }

    #[tokio::test]
    async fn mute_sync_fails_closed_and_clears_queued_messages() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let dispatcher = OscChatboxDispatcher::new(OscConfig {
            enabled: true,
            port: receiver.local_addr().unwrap().port(),
            mute_sync_enabled: true,
            mute_status_toast_enabled: false,
            preserve_original_text: true,
            translation_strategy: "preferred_only".into(),
        });

        assert_eq!(dispatcher.queue_test(), Err("osc.blocked_mute_unknown"));
        dispatcher.update_mute_status(Some(false));
        dispatcher.publish_subtitle(subtitle(1, "microphone", "stale"), false);
        dispatcher.update_mute_status(Some(true));
        assert_eq!(dispatcher.queue_test(), Err("osc.blocked_vrchat_muted"));
        assert_eq!(dispatcher.status().send_gate, "blocked_vrchat_muted");

        let received = tokio::time::timeout(Duration::from_millis(250), async {
            let mut buffer = [0u8; 512];
            receiver.recv_from(&mut buffer).await
        })
        .await;
        assert!(received.is_err(), "muting must discard queued messages");
    }
}
