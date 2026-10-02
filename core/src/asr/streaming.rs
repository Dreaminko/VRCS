use std::{collections::VecDeque, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::config::{ApiProfile, AsrConfig};
use crate::providers::{self, RecognitionTransport};

#[cfg(test)]
use crate::providers::{
    ALIBABA_PROVIDER, ALIBABA_TOKEN_PLAN_PROVIDER, CAPABILITY_SPEECH_TO_TEXT, OPENAI_PROVIDER,
    SERVICE_FUN_ASR_REALTIME, SERVICE_OPENAI_REALTIME, SERVICE_QWEN_REALTIME,
    SERVICE_TOKEN_PLAN_REALTIME,
};

use super::{read_credential, SharedAudio};

mod alignment;
mod idle;
mod provider;

pub use provider::SegmentationMode;
use provider::{InitializationEvent, NormalizationState, Provider};

#[cfg(test)]
use tokio_tungstenite::tungstenite::http::Request;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, PartialEq)]
pub struct LiveTranslationResult {
    /// Whether the native translation is still streaming.
    pub pending: bool,
    /// Source sentence IDs covered by this translation.
    pub source_utterance_ids: Vec<String>,
    pub provider: String,
    pub transcript: crate::models::LiveTranslation,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CloudEvent {
    LiveTranslation {
        service: String,
        snapshot: crate::models::LiveTranslation,
        completed: Vec<LiveTranslationResult>,
        translations: Vec<LiveTranslationResult>,
    },
    Partial {
        utterance_id: String,
        text: String,
        language: Option<String>,
    },
    Final {
        utterance_id: String,
        text: String,
        language: Option<String>,
    },
    Failed {
        utterance_id: Option<String>,
        reset_session: bool,
        code: String,
        detail: String,
    },
}

enum StreamingInput {
    Audio(SharedAudio),
    AudioActivity(SharedAudio, bool),
    Commit(oneshot::Sender<Result<(), String>>),
}

pub struct StreamingSession {
    audio: mpsc::Sender<StreamingInput>,
    events: mpsc::Receiver<CloudEvent>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
    segmentation_mode: SegmentationMode,
}

impl StreamingSession {
    pub async fn send(&self, samples: SharedAudio) -> Result<(), String> {
        self.audio
            .send(StreamingInput::Audio(samples))
            .await
            .map_err(|_| "Cloud recognition session is closed".to_string())
    }

    pub async fn send_with_activity(
        &self,
        samples: SharedAudio,
        speech: bool,
    ) -> Result<(), String> {
        if self.segmentation_mode != SegmentationMode::Continuous {
            return self.send(samples).await;
        }
        self.audio
            .send(StreamingInput::AudioActivity(samples, speech))
            .await
            .map_err(|_| "Cloud recognition session is closed".to_string())
    }

    pub async fn commit(&self) -> Result<(), String> {
        let (result_tx, result_rx) = oneshot::channel();
        self.audio
            .send(StreamingInput::Commit(result_tx))
            .await
            .map_err(|_| "Cloud recognition session is closed".to_string())?;
        result_rx
            .await
            .map_err(|_| "Cloud recognition session is closed".to_string())?
    }

    pub fn segmentation_mode(&self) -> SegmentationMode {
        self.segmentation_mode
    }

    pub async fn recv(&mut self) -> Option<CloudEvent> {
        self.events.recv().await
    }

    pub async fn stop(self) {
        self.stop_and_drain().await;
    }

    pub async fn stop_and_drain(mut self) -> Vec<CloudEvent> {
        let _ = self.stop.send(true);
        let mut drained = Vec::new();
        while let Some(event) = self.events.recv().await {
            drained.push(event);
        }
        let _ = self.task.await;
        drained
    }
}

fn active_profile(config: &AsrConfig) -> Result<&ApiProfile, String> {
    let active_id = config
        .active_profile_id
        .as_deref()
        .ok_or_else(|| format!("No API profile is selected for service {}", config.backend))?;
    let profile = config
        .api_profiles
        .iter()
        .find(|profile| profile.id == active_id)
        .ok_or_else(|| "The active API profile does not exist".to_string())?;
    providers::resolve_profile_service(profile, &config.backend)?;
    Ok(profile)
}

pub fn validate_cloud_connection(config: &AsrConfig) -> Result<(), String> {
    let provider = Provider::from_config(config)?;
    let profile = active_profile(config)?;
    let key = read_credential(&profile.id, &profile.provider)?
        .ok_or_else(|| format!("API key is not configured for {}", profile.name))?;
    provider.build_request(config, profile, &key).map(|_| ())
}

pub async fn spawn_streaming_session(
    config: AsrConfig,
    silence_seconds: f64,
) -> Result<StreamingSession, String> {
    let provider = Provider::from_config(&config)?;
    let profile = active_profile(&config)?.clone();
    let key = read_credential(&profile.id, &profile.provider)?
        .ok_or_else(|| format!("API key is not configured for {}", profile.name))?;
    let (socket, task_id) =
        connect_initialized(provider, &config, &profile, silence_seconds, &key).await?;
    let (audio_tx, audio_rx) = mpsc::channel(32);
    let (event_tx, event_rx) = mpsc::channel(32);
    let (stop_tx, stop_rx) = watch::channel(false);
    let task_config = config.clone();
    let task = tokio::spawn(async move {
        run_with_reconnect(
            provider,
            task_config,
            profile,
            silence_seconds,
            key,
            socket,
            task_id,
            audio_rx,
            event_tx,
            stop_rx,
        )
        .await;
    });
    Ok(StreamingSession {
        audio: audio_tx,
        events: event_rx,
        stop: stop_tx,
        task,
        segmentation_mode: provider.segmentation_mode(),
    })
}

fn resolve_test_service(
    profile: &ApiProfile,
    configured_service: &str,
    requested_service: Option<&str>,
) -> Result<String, String> {
    if let Some(service_id) = requested_service {
        let resolved = providers::resolve_profile_service(profile, service_id)?;
        if resolved.service.recognition_transport != Some(RecognitionTransport::RealtimeStream) {
            return Err(format!(
                "Service {service_id} is not a realtime cloud recognition service"
            ));
        }
        Provider::from_service(service_id)?;
        return Ok(service_id.to_owned());
    }

    if providers::resolve_profile_service(profile, configured_service).is_ok_and(|resolved| {
        resolved.service.recognition_transport == Some(RecognitionTransport::RealtimeStream)
            && Provider::from_service(configured_service).is_ok()
    }) {
        return Ok(configured_service.to_owned());
    }

    let definition = providers::definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    definition
        .services
        .iter()
        .find(|service| {
            service.recognition_transport == Some(RecognitionTransport::RealtimeStream)
                && Provider::from_service(service.id).is_ok()
                && providers::resolve_profile_service(profile, service.id).is_ok()
        })
        .map(|service| service.id.to_owned())
        .ok_or_else(|| {
            format!(
                "API profile {} does not support realtime speech recognition",
                profile.id
            )
        })
}

pub fn streaming_test_backend(
    config: &AsrConfig,
    profile_id: &str,
    service_id: Option<&str>,
) -> Result<String, String> {
    let profile = config
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "API profile does not exist".to_string())?;
    resolve_test_service(profile, &config.backend, service_id)
}

pub async fn test_streaming_connection(
    config: &AsrConfig,
    profile_id: &str,
    service_id: Option<&str>,
) -> Result<(), String> {
    let profile = config
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "API profile does not exist".to_string())?;
    let key = read_credential(&profile.id, &profile.provider)?
        .ok_or_else(|| format!("API key is not configured for {}", profile.name))?;
    let mut test_config = config.clone();
    test_config.backend = streaming_test_backend(config, profile_id, service_id)?;
    test_config.active_profile_id = Some(profile.id.clone());
    let provider = Provider::from_config(&test_config)?;
    let (mut socket, task_id) =
        connect_initialized(provider, &test_config, profile, 0.4, &key).await?;
    let (events, _) = mpsc::channel(1);
    finish(
        provider,
        &mut socket,
        &test_config,
        task_id.as_deref(),
        &mut NormalizationState::default(),
        &events,
        false,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_with_reconnect(
    provider: Provider,
    config: AsrConfig,
    profile: ApiProfile,
    silence_seconds: f64,
    key: String,
    mut socket: Socket,
    mut task_id: Option<String>,
    mut audio: mpsc::Receiver<StreamingInput>,
    events: mpsc::Sender<CloudEvent>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = Duration::from_millis(500);
    let mut replay = VecDeque::new();
    loop {
        let outcome = run_session_buffered(
            provider,
            &config,
            &mut socket,
            task_id.as_deref(),
            &mut audio,
            &events,
            &mut stop,
            &mut replay,
        )
        .await;
        let outcome = match outcome {
            Ok(SessionEnd::Idle(mut resume)) => {
                while !resume.ready && !*stop.borrow() && !resume.closed {
                    tokio::select! {
                        _ = stop.changed() => {}
                        input = audio.recv() => match input { Some(input) => resume.push(input), None => { resume.closed = true; break; } },
                    }
                }
                if !resume.ready {
                    break;
                }
                // Planned resumption is independent of the network failure policy.
                let mut resume_backoff = Duration::from_millis(500);
                let connection = loop {
                    let connection = idle::collect_until(
                        connect_initialized(provider, &config, &profile, silence_seconds, &key),
                        &mut audio,
                        &mut resume,
                    )
                    .await;
                    match connection {
                        Ok(connection) => break Ok(connection),
                        Err(error) => {
                            if config.cloud_failure_policy != "reconnect"
                                || *stop.borrow()
                                || !resume.has_capacity()
                            {
                                break Err(error);
                            }
                            let _ = events
                                .send(CloudEvent::Failed {
                                    utterance_id: None,
                                    reset_session: true,
                                    code: "asr.cloud_reconnect_failed".into(),
                                    detail: error,
                                })
                                .await;
                            idle::collect_until(
                                tokio::time::sleep(resume_backoff),
                                &mut audio,
                                &mut resume,
                            )
                            .await;
                            resume_backoff = (resume_backoff * 2).min(Duration::from_secs(8));
                        }
                    }
                };
                match connection {
                    Ok((next_socket, next_task_id)) => {
                        socket = next_socket;
                        task_id = next_task_id;
                        replay.extend(resume.take());
                        tracing::info!("Live translation resumed with buffered speech onset");
                        continue;
                    }
                    Err(error) => {
                        let _ = events
                            .send(CloudEvent::Failed {
                                utterance_id: None,
                                reset_session: true,
                                code: "asr.cloud_reconnect_failed".into(),
                                detail: error,
                            })
                            .await;
                        break;
                    }
                }
            }
            other => other,
        };
        if *stop.borrow() || audio.is_closed() {
            if let Err(detail) = outcome {
                let _ = events
                    .send(CloudEvent::Failed {
                        utterance_id: None,
                        reset_session: true,
                        code: "asr.cloud_drain_failed".into(),
                        detail,
                    })
                    .await;
            }
            break;
        }
        let detail = match outcome {
            Ok(_) => "Cloud recognition connection was closed".to_string(),
            Err(error) => error,
        };
        let _ = events
            .send(CloudEvent::Failed {
                utterance_id: None,
                reset_session: true,
                code: "asr.cloud_disconnected".into(),
                detail,
            })
            .await;
        if config.cloud_failure_policy != "reconnect" {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = stop.changed() => break,
        }
        match connect_initialized(provider, &config, &profile, silence_seconds, &key).await {
            Ok((next_socket, next_task_id)) => {
                if provider == Provider::OpenAiLiveTranslate {
                    while let Ok(input) = audio.try_recv() {
                        if let StreamingInput::Commit(result) = input {
                            let _ = result.send(Err("Translation session reconnected".into()));
                        }
                    }
                }
                socket = next_socket;
                task_id = next_task_id;
                backoff = Duration::from_millis(500);
            }
            Err(error) => {
                let _ = events
                    .send(CloudEvent::Failed {
                        utterance_id: None,
                        reset_session: true,
                        code: "asr.cloud_reconnect_failed".into(),
                        detail: error,
                    })
                    .await;
                backoff = (backoff * 2).min(Duration::from_secs(8));
            }
        }
    }
}

enum SessionEnd {
    Closed,
    Idle(idle::ResumeBuffer),
}

#[cfg(test)]
async fn run_session(
    provider: Provider,
    config: &AsrConfig,
    socket: &mut Socket,
    task_id: Option<&str>,
    audio: &mut mpsc::Receiver<StreamingInput>,
    events: &mpsc::Sender<CloudEvent>,
    stop: &mut watch::Receiver<bool>,
) -> Result<SessionEnd, String> {
    run_session_buffered(
        provider,
        config,
        socket,
        task_id,
        audio,
        events,
        stop,
        &mut VecDeque::new(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_session_buffered(
    provider: Provider,
    config: &AsrConfig,
    socket: &mut Socket,
    task_id: Option<&str>,
    audio: &mut mpsc::Receiver<StreamingInput>,
    events: &mpsc::Sender<CloudEvent>,
    stop: &mut watch::Receiver<bool>,
    replay: &mut VecDeque<StreamingInput>,
) -> Result<SessionEnd, String> {
    let mut normalization = NormalizationState::default();
    let outcome = run_session_inner(
        provider,
        config,
        socket,
        task_id,
        audio,
        events,
        stop,
        &mut normalization,
        replay,
    )
    .await;
    if matches!(
        provider,
        Provider::OpenAiLiveTranslate | Provider::GeminiLiveTranslate
    ) && outcome.is_err()
    {
        if let Some(event) = provider.finish_translation(config, &mut normalization) {
            let _ = events.send(event).await;
        }
    }
    outcome
}

fn alignment_worker(provider: Provider, config: &AsrConfig) -> Option<alignment::Worker> {
    (matches!(
        provider,
        Provider::OpenAiLiveTranslate | Provider::GeminiLiveTranslate
    ) && config.live_alignment.enabled)
        .then(|| alignment::Worker::new(config))
}

async fn run_session_inner(
    provider: Provider,
    config: &AsrConfig,
    socket: &mut Socket,
    task_id: Option<&str>,
    audio: &mut mpsc::Receiver<StreamingInput>,
    events: &mpsc::Sender<CloudEvent>,
    stop: &mut watch::Receiver<bool>,
    normalization: &mut NormalizationState,
    replay: &mut VecDeque<StreamingInput>,
) -> Result<SessionEnd, String> {
    let mut aligner = alignment_worker(provider, config);
    let mut audio_buffer = Vec::with_capacity(2048);
    let mut pending_audio = false;
    let mut activity = idle::Activity::default();
    let mut translation_tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            aligned = async {
                match aligner.as_mut() {
                    Some(worker) => worker.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some((window, Ok(mapping))) = aligned {
                    if let Some(event) = normalization.confirm_alignment(config, &window, &mapping) {
                        if events.send(event).await.is_err() { return Ok(SessionEnd::Closed); }
                    }
                }
            }
            _ = translation_tick.tick(), if provider.segmentation_mode() == SegmentationMode::Continuous => {
                if let Some(event) = provider.poll_translation(config, normalization) {
                    if events.send(event).await.is_err() { return Ok(SessionEnd::Closed); }
                }
                if let Some(aligner) = aligner.as_mut() {
                    aligner.try_start(|| normalization.alignment_window());
                }
            }
            _ = async { if provider.segmentation_mode() != SegmentationMode::Continuous || !*stop.borrow() { let _ = stop.changed().await; } } => {
                if matches!(provider, Provider::OpenAiLiveTranslate | Provider::GeminiLiveTranslate) {
                    audio.close();
                    while let Some(input) = match replay.pop_front() { Some(input) => Some(input), None => audio.recv().await } {
                        match input {
                            StreamingInput::Audio(samples) | StreamingInput::AudioActivity(samples, _) => {
                                pending_audio = true;
                                audio_buffer.extend_from_slice(&samples);
                                while let Some(packet) = take_audio_packet(&mut audio_buffer, provider.audio_packet_samples()) {
                                    send_audio(provider, socket, packet).await?;
                                }
                            }
                            StreamingInput::Commit(result) => { let _ = result.send(Ok(())); }
                        }
                    }
                }
                flush_audio_buffer(provider, socket, &mut audio_buffer).await?;
                if pending_audio {
                    let _ = commit_utterance(provider, socket).await;
                }
                return finish_aligned(provider, socket, config, task_id, normalization, events, pending_audio, aligner.as_mut()).await.map(|()| SessionEnd::Closed);
            }
            input = async { if replay.is_empty() { audio.recv().await } else { None } } => {
                let input = if replay.is_empty() { input } else { replay.pop_front() };
                match input {
                    Some(input @ (StreamingInput::Audio(_) | StreamingInput::AudioActivity(_, _))) => {
                        let (samples, speech) = match input {
                            StreamingInput::Audio(samples) => (samples, true),
                            StreamingInput::AudioActivity(samples, speech) => (samples, speech),
                            _ => unreachable!(),
                        };
                        let idle = provider.segmentation_mode() == SegmentationMode::Continuous && activity.push(samples.len(), speech);
                        pending_audio = true;
                        audio_buffer.extend_from_slice(samples.as_slice());
                        while let Some(packet) = take_audio_packet(&mut audio_buffer, provider.audio_packet_samples()) {
                            send_audio(provider, socket, packet).await?;
                        }
                        if idle {
                            flush_audio_buffer(provider, socket, &mut audio_buffer).await?;
                            let mut resume = idle::ResumeBuffer::default();
                            let closing = finish_aligned(provider, socket, config, task_id, normalization, events, pending_audio, aligner.as_mut());
                            if let Err(error) = idle::collect_until(closing, audio, &mut resume).await {
                                // A failed close must not discard speech captured for the next session.
                                if let Some(event) = provider.finish_translation(config, normalization) {
                                    let _ = events.send(event).await;
                                }
                                let _ = socket.close(None).await;
                                tracing::warn!(%error, "Idle translation close incomplete; retained received text and resume audio");
                            }
                            tracing::info!("Live translation suspended after 30 seconds without speech");
                            return Ok(SessionEnd::Idle(resume));
                        }
                    }
                    Some(StreamingInput::Commit(result)) => {
                        let commit = async {
                            flush_audio_buffer(provider, socket, &mut audio_buffer).await?;
                            commit_utterance(provider, socket).await
                        }
                        .await;
                        match commit {
                            Ok(()) => {
                                pending_audio = false;
                                let _ = result.send(Ok(()));
                            }
                            Err(error) => {
                                let _ = result.send(Err(error.clone()));
                                return Err(error);
                            }
                        }
                    }
                    None => {
                        flush_audio_buffer(provider, socket, &mut audio_buffer).await?;
                        if pending_audio {
                            let _ = commit_utterance(provider, socket).await;
                        }
                        return finish_aligned(provider, socket, config, task_id, normalization, events, pending_audio, aligner.as_mut()).await.map(|()| SessionEnd::Closed);
                    }
                }
            }
            message = receive_event(socket) => {
                let value = message?;
                if provider == Provider::OpenAiLiveTranslate && provider.is_finished(&value) {
                    if let Some(event) = provider.finish_translation(config, normalization) {
                        let _ = events.send(event).await;
                    }
                    return Err("OpenAI closed the translation session".into());
                }
                let source_len = normalization.live_source_len();
                if let Some(event) = provider.normalize_event(config, &value, normalization)? {
                    // Actual source transcript growth protects quiet speech missed by local VAD.
                    if provider.segmentation_mode() == SegmentationMode::Continuous && normalization.live_source_len() > source_len {
                        activity.push(0, true);
                    }
                    if events.send(event).await.is_err() {
                        return Ok(SessionEnd::Closed);
                    }
                }
            }
        }
    }
}

async fn connect(
    provider: Provider,
    config: &AsrConfig,
    profile: &ApiProfile,
    key: &str,
) -> Result<Socket, String> {
    let request = provider.build_request(config, profile, key)?;
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .map_err(|error| provider.connection_error(error))
}

async fn connect_initialized(
    provider: Provider,
    config: &AsrConfig,
    profile: &ApiProfile,
    silence_seconds: f64,
    key: &str,
) -> Result<(Socket, Option<String>), String> {
    let mut socket = tokio::time::timeout(
        Duration::from_secs(10),
        connect(provider, config, profile, key),
    )
    .await
    .map_err(|_| "Timed out while connecting to cloud recognition service".to_string())??;
    let task_id = provider.task_id();
    let update = provider.start_message(config, silence_seconds, task_id.as_deref())?;
    initialize_socket(provider, &mut socket, update).await?;
    Ok((socket, task_id))
}

async fn initialize_socket(
    provider: Provider,
    socket: &mut Socket,
    update: Value,
) -> Result<(), String> {
    socket
        .send(Message::Text(update.to_string().into()))
        .await
        .map_err(|error| format!("Failed to initialize cloud recognition session: {error}"))?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match provider.initialization_event(&receive_event(socket).await?, &update) {
                InitializationEvent::Ready => return Ok(()),
                InitializationEvent::Failed(detail) => return Err(detail),
                InitializationEvent::Pending => {}
            }
        }
    })
    .await
    .map_err(|_| "Timed out waiting for cloud recognition session confirmation".to_string())?
}

async fn receive_event(socket: &mut Socket) -> Result<Value, String> {
    loop {
        let message = socket
            .next()
            .await
            .ok_or_else(|| "Cloud recognition connection was closed".to_string())?
            .map_err(|error| format!("Failed to read cloud recognition event: {error}"))?;
        let parsed = match message {
            Message::Text(text) => serde_json::from_str(&text),
            Message::Binary(bytes) => serde_json::from_slice(&bytes),
            Message::Close(frame) => {
                let detail = frame
                    .map(|frame| format!(": {} {}", frame.code, frame.reason))
                    .unwrap_or_default();
                return Err(format!(
                    "Cloud recognition service closed the connection{detail}"
                ));
            }
            Message::Ping(value) => {
                socket
                    .send(Message::Pong(value))
                    .await
                    .map_err(|error| format!("Cloud recognition heartbeat failed: {error}"))?;
                continue;
            }
            _ => continue,
        };
        return parsed.map_err(|error| format!("Cloud recognition returned invalid JSON: {error}"));
    }
}

#[cfg(test)]
fn normalize_event(
    provider: Provider,
    config: &AsrConfig,
    message: &str,
    state: &mut NormalizationState,
) -> Result<Option<CloudEvent>, String> {
    let value: Value = serde_json::from_str(message)
        .map_err(|error| format!("Cloud recognition returned invalid JSON: {error}"))?;
    provider.normalize_event(config, &value, state)
}

#[cfg(test)]
const AUDIO_PACKET_SAMPLES: usize = 1600;

fn take_audio_packet(buffer: &mut Vec<f32>, packet_samples: usize) -> Option<Vec<f32>> {
    (buffer.len() >= packet_samples).then(|| buffer.drain(..packet_samples).collect())
}

fn take_buffered_audio(buffer: &mut Vec<f32>) -> Option<Vec<f32>> {
    (!buffer.is_empty()).then(|| std::mem::take(buffer))
}

async fn flush_audio_buffer(
    provider: Provider,
    socket: &mut Socket,
    buffer: &mut Vec<f32>,
) -> Result<(), String> {
    let Some(samples) = take_buffered_audio(buffer) else {
        return Ok(());
    };
    send_audio(provider, socket, samples).await
}

async fn send_audio(
    provider: Provider,
    socket: &mut Socket,
    samples: Vec<f32>,
) -> Result<(), String> {
    socket
        .send(provider.audio_message(&samples))
        .await
        .map_err(|error| format!("Failed to send cloud recognition audio: {error}"))
}

async fn commit_utterance(provider: Provider, socket: &mut Socket) -> Result<(), String> {
    let Some(message) = provider.commit_message() else {
        return Ok(());
    };
    socket
        .send(message)
        .await
        .map_err(|error| format!("Failed to commit cloud recognition audio: {error}"))
}

async fn finish(
    provider: Provider,
    socket: &mut Socket,
    config: &AsrConfig,
    task_id: Option<&str>,
    state: &mut NormalizationState,
    events: &mpsc::Sender<CloudEvent>,
    had_audio: bool,
) -> Result<(), String> {
    finish_aligned(
        provider, socket, config, task_id, state, events, had_audio, None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn finish_aligned(
    provider: Provider,
    socket: &mut Socket,
    config: &AsrConfig,
    task_id: Option<&str>,
    state: &mut NormalizationState,
    events: &mpsc::Sender<CloudEvent>,
    had_audio: bool,
    aligner: Option<&mut alignment::Worker>,
) -> Result<(), String> {
    if provider == Provider::GeminiLiveTranslate && had_audio {
        // The continuous translator needs audio frames to emit its buffered tail.
        // audioStreamEnd alone did not flush the final words in live probes.
        for _ in 0..5 {
            send_audio(provider, socket, vec![0.0; 1600]).await?;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    if let Some(message) = provider.finish_message(task_id) {
        socket
            .send(message)
            .await
            .map_err(|e| format!("Failed to close cloud recognition session: {e}"))?;
    }
    let deadline = tokio::time::sleep(Duration::from_secs(
        if matches!(
            provider,
            Provider::GeminiLiveTranslate | Provider::OpenAiLiveTranslate
        ) {
            5
        } else {
            3
        },
    ));
    tokio::pin!(deadline);
    let outcome = loop {
        tokio::select! {
            _ = &mut deadline => break Err("Timed out waiting for cloud recognition to finish".to_owned()),
            message = receive_event(socket) => {
                let value = match message { Ok(value) => value, Err(error) => break Err(error) };
                let finished = provider.is_finished(&value);
                match provider.normalize_event(config, &value, state) {
                    Ok(Some(event)) => { let _ = events.send(event).await; }
                    Ok(None) => {}
                    Err(error) => break Err(error),
                }
                if finished { break Ok(()); }
            }
        }
    };
    let _ = socket.close(None).await;
    if let Some(worker) = aligner {
        // Replace obsolete work with one latest-tail request under a separate budget.
        let _ = tokio::time::timeout(Duration::from_secs(4), async {
            worker.cancel_running();
            worker.start_final(|| state.alignment_window());
            if worker.is_running() {
                if let Some((window, Ok(mapping))) = worker.recv().await {
                    if let Some(event) = state.apply_alignment(config, &window, &mapping) {
                        let _ = events.send(event).await;
                    }
                }
            }
        })
        .await;
        worker.cancel_running();
    }
    // Preserve buffered native text even when close or final alignment times out.
    if let Some(event) = provider.finish_translation(config, state) {
        let _ = events.send(event).await;
    }
    if provider == Provider::OpenAiLiveTranslate {
        outcome
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn build_request(
    config: &AsrConfig,
    profile: &ApiProfile,
    key: &str,
) -> Result<Request<()>, String> {
    Provider::from_config(config)?.build_request(config, profile, key)
}

#[cfg(test)]
fn session_update(config: &AsrConfig, silence_seconds: f64) -> Value {
    Provider::from_config(config)
        .unwrap()
        .start_message(config, silence_seconds, None)
        .unwrap()
}

#[cfg(test)]
fn fun_run_task(config: &AsrConfig, silence_seconds: f64, task_id: &str) -> Value {
    Provider::FunAsr
        .start_message(config, silence_seconds, Some(task_id))
        .unwrap()
}

#[cfg(test)]
fn pcm16_bytes(samples: &[f32]) -> Vec<u8> {
    provider::pcm16_bytes(samples)
}

#[cfg(test)]
fn resample_16k_to_24k(samples: &[f32]) -> Vec<f32> {
    provider::resample_16k_to_24k(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_delta(provider: Provider, kind: &str, text: &str) -> Value {
        if provider == Provider::OpenAiLiveTranslate {
            serde_json::json!({"type":format!("session.{kind}_transcript.delta"),"delta":text})
        } else {
            serde_json::json!({"serverContent":{format!("{kind}Transcription"):{"text":text}}})
        }
    }

    #[tokio::test]
    async fn final_alignment_uses_drained_tail_and_preview_precedes_model_completion() {
        use axum::{extract::Json, routing::post, Router};
        for provider in [Provider::OpenAiLiveTranslate, Provider::GeminiLiveTranslate] {
            let (requested, mut requests) = mpsc::channel(1);
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            let gate = release.clone();
            let app = Router::new().route("/v1/chat/completions", post(move |Json(body): Json<Value>| {
                let requested = requested.clone(); let gate = gate.clone();
                async move {
                    let w: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                    assert_eq!(w["sources"][0]["text"], "Hello. Next.");
                    assert_eq!(w["targets"][0]["text"], "你好。下一句。");
                    requested.send(()).await.unwrap(); gate.notified().await;
                    Json(serde_json::json!({"choices":[{"message":{"content":serde_json::json!({"groups":[
                        {"source_end":{"unit_id":w["sources"][0]["id"],"quote":"Hello."},"target_end":{"unit_id":w["targets"][0]["id"],"quote":"你好。"},"fully_translated":true},
                        {"source_end":{"unit_id":w["sources"][0]["id"],"quote":"Next."},"target_end":{"unit_id":w["targets"][0]["id"],"quote":"下一句。"},"fully_translated":true}
                    ]}).to_string()}}]}))
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let http = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let mut config = translation_config();
            if provider == Provider::GeminiLiveTranslate {
                config.backend = crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE.into();
            }
            config.api_profiles.push(ApiProfile {
                id: "local".into(),
                provider: crate::providers::OLLAMA_PROVIDER.into(),
                base_url: Some(format!("http://{address}/v1")),
                auth_mode: crate::config::ApiAuthMode::None,
                ..Default::default()
            });
            config.live_alignment.profile_id = Some("local".into());
            config.live_alignment.enabled = true;
            let mut worker = alignment::Worker::new(&config);
            let mut state = NormalizationState::default();
            for (kind, text) in [("input", "Hello."), ("output", "你好。")] {
                provider
                    .normalize_event(&config, &native_delta(provider, kind, text), &mut state)
                    .unwrap();
            }
            let (mut client, mut server) = socket_pair().await;
            let (events, mut received) = mpsc::channel(8);
            let task = tokio::spawn(async move {
                finish_aligned(
                    provider,
                    &mut client,
                    &config,
                    None,
                    &mut state,
                    &events,
                    false,
                    Some(&mut worker),
                )
                .await
            });
            let close = server.next().await.unwrap().unwrap();
            assert!(close.to_text().unwrap().contains(
                if provider == Provider::OpenAiLiveTranslate {
                    "session.close"
                } else {
                    "audioStreamEnd"
                }
            ));
            for (kind, text) in [("input", " Next."), ("output", "下一句。")] {
                server
                    .send(json_frame(native_delta(provider, kind, text), false))
                    .await
                    .unwrap();
            }
            if provider == Provider::OpenAiLiveTranslate {
                server
                    .send(json_frame(
                        serde_json::json!({"type":"session.closed"}),
                        false,
                    ))
                    .await
                    .unwrap();
            } else {
                server.close(None).await.unwrap();
            }
            tokio::time::timeout(Duration::from_secs(2), requests.recv())
                .await
                .unwrap()
                .unwrap();
            let mut previews = Vec::new();
            while let Ok(CloudEvent::LiveTranslation {
                snapshot,
                completed,
                ..
            }) = received.try_recv()
            {
                assert!(completed.is_empty());
                previews.push(snapshot.translation);
            }
            assert!(previews.contains(&"你好。下一句。".into()));
            release.notify_one();
            task.await.unwrap().unwrap();
            let mut translations = Vec::new();
            while let Some(CloudEvent::LiveTranslation {
                translations: updates,
                ..
            }) = received.recv().await
            {
                translations.extend(updates);
            }
            assert_eq!(
                translations
                    .iter()
                    .map(|t| t.transcript.translation.as_str())
                    .collect::<Vec<_>>(),
                ["你好。", "下一句。"]
            );
            http.abort();
        }
    }

    #[tokio::test]
    async fn final_alignment_timeout_preserves_unresolved_text_exactly() {
        use axum::{extract::Json, routing::post, Router};
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                Json(serde_json::json!({"choices":[{"message":{"content":"{\"groups\":[]}"}}]}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let http = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let provider = Provider::OpenAiLiveTranslate;
        let mut config = translation_config();
        config.api_profiles.push(ApiProfile {
            id: "local".into(),
            provider: crate::providers::OLLAMA_PROVIDER.into(),
            base_url: Some(format!("http://{address}/v1")),
            auth_mode: crate::config::ApiAuthMode::None,
            timeout_ms: 1000,
            ..Default::default()
        });
        config.live_alignment.profile_id = Some("local".into());
        config.live_alignment.enabled = true;
        let mut worker = alignment::Worker::new(&config);
        let mut state = NormalizationState::default();
        for (kind, text) in [
            ("input", " Hello.\n Next."),
            ("output", " 你好。\n 下一句。 "),
        ] {
            provider
                .normalize_event(&config, &native_delta(provider, kind, text), &mut state)
                .unwrap();
        }
        let (mut client, mut server) = socket_pair().await;
        let (events, mut received) = mpsc::channel(8);
        let task = tokio::spawn(async move {
            finish_aligned(
                provider,
                &mut client,
                &config,
                None,
                &mut state,
                &events,
                false,
                Some(&mut worker),
            )
            .await
            .unwrap();
            assert!(!worker.is_running());
        });
        server.next().await.unwrap().unwrap();
        server
            .send(json_frame(
                serde_json::json!({"type":"session.closed"}),
                false,
            ))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        let Some(CloudEvent::LiveTranslation { translations, .. }) = received.recv().await else {
            panic!()
        };
        assert_eq!(translations.len(), 1);
        assert_eq!(translations[0].transcript.text, " Hello.\n Next.");
        assert_eq!(
            translations[0].transcript.translation,
            " 你好。\n 下一句。 "
        );
        assert!(received.recv().await.is_none());
        http.abort();
    }

    #[tokio::test]
    async fn long_silence_closes_live_sessions_normally_and_retains_native_tail() {
        for provider in [Provider::OpenAiLiveTranslate, Provider::GeminiLiveTranslate] {
            let (mut client, mut server) = socket_pair().await;
            let (tx, mut rx) = mpsc::channel(2);
            let (_stop, mut stop_rx) = watch::channel(false);
            let (events, mut received) = mpsc::channel(8);
            tx.send(StreamingInput::AudioActivity(
                std::sync::Arc::new(vec![0.1; 512]),
                true,
            ))
            .await
            .unwrap();
            tx.send(StreamingInput::AudioActivity(
                std::sync::Arc::new(vec![0.0; idle::IDLE_SAMPLES]),
                false,
            ))
            .await
            .unwrap();
            let task = tokio::spawn(async move {
                let mut config = translation_config();
                if provider == Provider::GeminiLiveTranslate {
                    config.backend = crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE.into();
                }
                run_session(
                    provider,
                    &config,
                    &mut client,
                    None,
                    &mut rx,
                    &events,
                    &mut stop_rx,
                )
                .await
            });
            let mut packets = 0;
            loop {
                let frame = server.next().await.unwrap().unwrap();
                let text = frame.to_text().unwrap();
                if text.contains("session.close") || text.contains("audioStreamEnd") {
                    break;
                }
                packets += 1;
            }
            assert!(packets > 100);
            for (kind, text) in [("input", "Tail."), ("output", "尾句。")] {
                server
                    .send(json_frame(native_delta(provider, kind, text), false))
                    .await
                    .unwrap();
            }
            if provider == Provider::OpenAiLiveTranslate {
                server
                    .send(json_frame(
                        serde_json::json!({"type":"session.closed"}),
                        false,
                    ))
                    .await
                    .unwrap();
            } else {
                server.close(None).await.unwrap();
            }
            let outcome = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(matches!(outcome, SessionEnd::Idle(_)));
            let mut tails = Vec::new();
            while let Some(CloudEvent::LiveTranslation { translations, .. }) = received.recv().await
            {
                tails.extend(translations);
            }
            assert_eq!(tails.len(), 1);
            assert_eq!(tails[0].transcript.translation, "尾句。");
        }
    }

    #[tokio::test]
    async fn alignment_worker_is_created_only_for_enabled_live_translation() {
        let mut config = AsrConfig::default();
        for enabled in [false, true] {
            config.live_alignment.enabled = enabled;
            for provider in [
                Provider::Qwen,
                Provider::TokenPlan,
                Provider::FunAsr,
                Provider::OpenAi,
                Provider::Gemini,
            ] {
                assert!(alignment_worker(provider, &config).is_none());
            }
            for provider in [Provider::OpenAiLiveTranslate, Provider::GeminiLiveTranslate] {
                assert_eq!(alignment_worker(provider, &config).is_some(), enabled);
            }
        }
    }

    #[tokio::test]
    async fn abrupt_gemini_disconnect_preserves_received_native_text() {
        let (mut client, mut server) = socket_pair().await;
        let (_audio_tx, mut audio_rx) = mpsc::channel(1);
        let (_stop_tx, mut stop_rx) = watch::channel(false);
        let (events, mut received) = mpsc::channel(8);
        let task = tokio::spawn(async move {
            let config = AsrConfig {
                backend: crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE.into(),
                live_translation_target: Some("zh-Hans".into()),
                ..Default::default()
            };
            run_session(
                Provider::GeminiLiveTranslate,
                &config,
                &mut client,
                None,
                &mut audio_rx,
                &events,
                &mut stop_rx,
            )
            .await
        });
        server
            .send(json_frame(
                serde_json::json!({"serverContent": {
                    "inputTranscription": {"text": "Original tail"},
                    "outputTranscription": {"text": "译文尾部"}
                }}),
                false,
            ))
            .await
            .unwrap();
        // Wait for the delta before closing, so the test checks a buffered tail.
        let first = tokio::time::timeout(Duration::from_secs(1), received.recv())
            .await
            .unwrap()
            .unwrap();
        let CloudEvent::LiveTranslation {
            service,
            completed,
            translations,
            snapshot,
        } = first
        else {
            panic!()
        };
        assert_eq!(service, crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE);
        assert_eq!(snapshot.text, "Original tail");
        assert!(completed.is_empty() && translations.is_empty());
        server.close(None).await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        let mut originals = Vec::new();
        let mut translated = Vec::new();
        while let Some(event) = received.recv().await {
            let CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            } = event
            else {
                panic!()
            };
            originals.extend(completed);
            translated.extend(translations);
        }
        assert_eq!(originals.len(), 1);
        assert_eq!(originals[0].transcript.text, "Original tail");
        assert_eq!(translated.len(), 1);
        assert_eq!(translated[0].transcript.translation, "译文尾部");
    }

    async fn socket_pair() -> (Socket, WebSocketStream<TcpStream>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (client, server) = tokio::join!(tokio_tungstenite::connect_async(url), async {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(stream).await.unwrap()
        });
        (client.unwrap().0, server)
    }

    fn json_frame(value: Value, binary: bool) -> Message {
        if binary {
            Message::Binary(serde_json::to_vec(&value).unwrap().into())
        } else {
            Message::Text(value.to_string().into())
        }
    }

    fn translation_config() -> AsrConfig {
        AsrConfig {
            backend: providers::SERVICE_OPENAI_REALTIME_TRANSLATE.into(),
            live_translation_target: Some("zh-Hant".into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn openai_translation_displays_without_target_punctuation_or_stop() {
        let (mut client, mut server) = socket_pair().await;
        let (_audio_tx, mut audio_rx) = mpsc::channel(1);
        let (events, mut received) = mpsc::channel(16);
        let (stop, mut stopped) = watch::channel(false);
        let task = tokio::spawn(async move {
            run_session(
                Provider::OpenAiLiveTranslate,
                &translation_config(),
                &mut client,
                None,
                &mut audio_rx,
                &events,
                &mut stopped,
            )
            .await
            .unwrap();
        });
        let exchange = async {
            for (kind, delta, frame) in [
                ("input_transcript", "Hello.", 200),
                ("output_transcript", "你好", 400),
                ("input_transcript", " How are you?", 1000),
                ("output_transcript", "最近怎么样", 1200),
            ] {
                server.send(json_frame(serde_json::json!({
                    "type": format!("session.{kind}.delta"), "delta": delta, "elapsed_ms": frame
                }), true)).await.unwrap();
            }
            let mut originals = Vec::new();
            loop {
                let CloudEvent::LiveTranslation {
                    snapshot,
                    completed,
                    translations,
                    ..
                } = received.recv().await.unwrap()
                else {
                    panic!()
                };
                originals.extend(completed);
                assert!(translations.is_empty());
                if snapshot.translation == "你好最近怎么样" {
                    break;
                }
            }
            // Display does not depend on punctuation, source silence, a key or model completion.
            stop.send(true).unwrap();
            let close = server.next().await.unwrap().unwrap();
            assert!(close.to_text().unwrap().contains("session.close"));
            server
                .send(json_frame(
                    serde_json::json!({"type":"session.closed"}),
                    false,
                ))
                .await
                .unwrap();
            task.await.unwrap();
            let mut updates = Vec::new();
            while let Ok(CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            }) = received.try_recv()
            {
                originals.extend(completed);
                updates.extend(translations.into_iter().filter(|update| !update.pending));
            }
            assert_eq!(originals.len(), 1);
            assert_eq!(originals[0].transcript.text, "Hello. How are you?");
            assert_eq!(updates.len(), 1);
            assert_eq!(updates[0].transcript.translation, "你好最近怎么样");
        };
        tokio::time::timeout(Duration::from_secs(4), exchange)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn openai_translation_sends_queued_audio_before_close_and_flushes_tail_once() {
        use base64::Engine as _;
        for binary in [false, true] {
            let (mut client, mut server) = socket_pair().await;
            let (audio_tx, audio_rx) = mpsc::channel(4);
            let (event_tx, event_rx) = mpsc::channel(1);
            let (stop_tx, stop_rx) = watch::channel(false);
            for size in [1600, 1600, 1600, 100] {
                audio_tx
                    .send(StreamingInput::Audio(std::sync::Arc::new(vec![0.0; size])))
                    .await
                    .unwrap();
            }
            let task = tokio::spawn(async move {
                let mut audio_rx = audio_rx;
                let mut stop_rx = stop_rx;
                run_session(
                    Provider::OpenAiLiveTranslate,
                    &translation_config(),
                    &mut client,
                    None,
                    &mut audio_rx,
                    &event_tx,
                    &mut stop_rx,
                )
                .await
                .unwrap();
            });
            let session = StreamingSession {
                audio: audio_tx,
                events: event_rx,
                stop: stop_tx,
                task,
                segmentation_mode: SegmentationMode::Continuous,
            };
            let exchange = async {
                let (events, ()) = tokio::join!(session.stop_and_drain(), async {
                    for size in [3200, 1700] {
                        let message = server.next().await.unwrap().unwrap();
                        let value: Value =
                            serde_json::from_str(message.to_text().unwrap()).unwrap();
                        assert_eq!(value["type"], "session.input_audio_buffer.append");
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(value["audio"].as_str().unwrap())
                            .unwrap();
                        assert_eq!(bytes, vec![0; size * 3]);
                    }
                    let close = server.next().await.unwrap().unwrap();
                    let close: Value = serde_json::from_str(close.to_text().unwrap()).unwrap();
                    assert_eq!(close["type"], "session.close");
                    server.send(Message::Ping(vec![1].into())).await.unwrap();
                    for (kind, delta, frame) in [
                        ("input_transcript", "Hello.", 200),
                        ("output_audio", "AA==", 400),
                        ("output_transcript", "你好。", 400),
                        ("input_transcript", " Tail", 1000),
                        ("output_transcript", "尾句", 1200),
                    ] {
                        server.send(json_frame(serde_json::json!({"type": format!("session.{kind}.delta"), "delta": delta, "elapsed_ms": frame}), binary)).await.unwrap();
                    }
                    server
                        .send(json_frame(
                            serde_json::json!({"type":"session.closed"}),
                            binary,
                        ))
                        .await
                        .unwrap();
                    while let Some(Ok(message)) = server.next().await {
                        match message {
                            Message::Close(_) => break,
                            Message::Pong(_) => {}
                            other => {
                                panic!("unexpected client message after session.close: {other:?}")
                            }
                        }
                    }
                });
                let mut results = Vec::new();
                let mut updates = Vec::new();
                for event in events {
                    let CloudEvent::LiveTranslation {
                        completed,
                        translations,
                        ..
                    } = event
                    else {
                        panic!()
                    };
                    results.extend(completed);
                    updates.extend(translations.into_iter().filter(|update| !update.pending));
                }
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].transcript.text, "Hello. Tail");
                assert_eq!(updates.len(), 1);
                assert_eq!(updates[0].transcript.translation, "你好。尾句");
                assert_eq!(
                    updates[0].source_utterance_ids,
                    results
                        .iter()
                        .map(|r| r.transcript.utterance_id.clone())
                        .collect::<Vec<_>>()
                );
            };
            tokio::time::timeout(Duration::from_secs(2), exchange)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn openai_translation_incomplete_drain_preserves_received_native_text() {
        for disconnect in [false, true] {
            let (mut client, mut server) = socket_pair().await;
            let config = translation_config();
            let mut state = NormalizationState::default();
            Provider::OpenAiLiveTranslate
                .normalize_event(
                    &config,
                    &serde_json::json!({"type":"session.input_transcript.delta","delta":"pending"}),
                    &mut state,
                )
                .unwrap();
            let (events, mut received) = mpsc::channel(4);
            let (result, ()) = tokio::join!(
                finish(
                    Provider::OpenAiLiveTranslate,
                    &mut client,
                    &config,
                    None,
                    &mut state,
                    &events,
                    true
                ),
                async {
                    server.next().await.unwrap().unwrap();
                    server.send(json_frame(serde_json::json!({"type":"session.output_transcript.delta","delta":"未完成"}), false)).await.unwrap();
                    if disconnect {
                        server.close(None).await.unwrap();
                    } else {
                        while let Some(Ok(message)) = server.next().await {
                            if matches!(message, Message::Close(_)) {
                                break;
                            }
                        }
                    }
                }
            );
            assert!(result.is_err());
            if !disconnect {
                assert!(result.unwrap_err().contains("Timed out"));
            }
            let mut originals = Vec::new();
            let mut updates = Vec::new();
            while let Ok(event) = received.try_recv() {
                let CloudEvent::LiveTranslation {
                    completed,
                    translations,
                    ..
                } = event
                else {
                    panic!()
                };
                originals.extend(completed);
                updates.extend(translations);
            }
            assert_eq!(originals.len(), 1);
            assert_eq!(originals[0].transcript.text, "pending");
            assert_eq!(updates[0].transcript.translation, "未完成");
            // A new session has no source text to pair with an old translation.
            let mut reconnected = NormalizationState::default();
            Provider::OpenAiLiveTranslate
                .normalize_event(
                    &config,
                    &serde_json::json!({"type":"session.output_transcript.delta","delta":"stale"}),
                    &mut reconnected,
                )
                .unwrap();
            if let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            }) = Provider::OpenAiLiveTranslate.finish_translation(&config, &mut reconnected)
            {
                assert!(completed.is_empty() && translations.is_empty());
            }
            assert!(Provider::OpenAiLiveTranslate
                .finish_translation(&config, &mut reconnected)
                .is_none());
        }
    }

    #[tokio::test]
    async fn abrupt_translation_disconnect_preserves_buffered_source_and_target() {
        let (mut client, mut server) = socket_pair().await;
        let (_audio_tx, mut audio_rx) = mpsc::channel(1);
        let (_stop_tx, mut stop_rx) = watch::channel(false);
        let (events, mut received) = mpsc::channel(8);
        let task = tokio::spawn(async move {
            run_session(
                Provider::OpenAiLiveTranslate,
                &translation_config(),
                &mut client,
                None,
                &mut audio_rx,
                &events,
                &mut stop_rx,
            )
            .await
        });
        for (kind, delta) in [("input", "Original tail"), ("output", "译文尾部")] {
            server.send(json_frame(serde_json::json!({"type":format!("session.{kind}_transcript.delta"),"delta":delta}),false)).await.unwrap();
        }
        server.close(None).await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        let mut originals = Vec::new();
        let mut updates = Vec::new();
        while let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = received.recv().await
        {
            originals.extend(completed);
            updates.extend(translations);
        }
        assert_eq!(originals.len(), 1);
        assert_eq!(originals[0].transcript.text, "Original tail");
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].transcript.translation, "译文尾部");
    }

    #[tokio::test]
    async fn stop_drains_a_full_event_channel_before_joining() {
        let (audio, _audio_rx) = mpsc::channel(1);
        let (event_tx, events) = mpsc::channel(1);
        let (stop, mut stop_rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            stop_rx.changed().await.unwrap();
            for index in 0..64 {
                event_tx
                    .send(CloudEvent::Partial {
                        utterance_id: index.to_string(),
                        text: "tail".into(),
                        language: None,
                    })
                    .await
                    .unwrap();
            }
        });
        let session = StreamingSession {
            audio,
            events,
            stop,
            task,
            segmentation_mode: SegmentationMode::Continuous,
        };
        let drained = tokio::time::timeout(Duration::from_secs(2), session.stop_and_drain())
            .await
            .unwrap();
        assert_eq!(drained.len(), 64);
    }

    #[tokio::test]
    async fn stopping_before_speech_sends_no_audio_for_either_live_translator() {
        for provider in [Provider::OpenAiLiveTranslate, Provider::GeminiLiveTranslate] {
            let (mut client, mut server) = socket_pair().await;
            let mut config = translation_config();
            if provider == Provider::GeminiLiveTranslate {
                config.backend = providers::SERVICE_GEMINI_LIVE_TRANSLATE.into();
            }
            let (_audio_tx, mut audio_rx) = mpsc::channel(4);
            let (stop_tx, mut stop_rx) = watch::channel(false);
            stop_tx.send(true).unwrap();
            let (events, _) = mpsc::channel(4);
            let task = tokio::spawn(async move {
                run_session(
                    provider,
                    &config,
                    &mut client,
                    None,
                    &mut audio_rx,
                    &events,
                    &mut stop_rx,
                )
                .await
            });
            let message = tokio::time::timeout(Duration::from_secs(1), server.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if provider == Provider::OpenAiLiveTranslate {
                assert_eq!(value["type"], "session.close");
                server
                    .send(json_frame(
                        serde_json::json!({"type":"session.closed"}),
                        false,
                    ))
                    .await
                    .unwrap();
            } else {
                assert_eq!(
                    value.pointer("/realtimeInput/audioStreamEnd"),
                    Some(&Value::Bool(true))
                );
                server.close(None).await.unwrap();
            }
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn gemini_translation_drains_audio_and_pads_before_ending_the_stream() {
        use base64::Engine as _;
        for explicit_stop in [false, true] {
            let (mut client, mut server) = socket_pair().await;
            let mut config = translation_config();
            config.backend = providers::SERVICE_GEMINI_LIVE_TRANSLATE.into();
            let (audio_tx, mut audio_rx) = mpsc::channel(4);
            for size in [1700, 3200] {
                audio_tx
                    .send(StreamingInput::Audio(std::sync::Arc::new(vec![0.5; size])))
                    .await
                    .unwrap();
            }
            let (stop_tx, mut stop_rx) = watch::channel(false);
            if explicit_stop {
                stop_tx.send(true).unwrap();
            } else {
                drop(audio_tx);
            }
            let (events, mut received) = mpsc::channel(16);
            let task = tokio::spawn(async move {
                run_session(
                    Provider::GeminiLiveTranslate,
                    &config,
                    &mut client,
                    None,
                    &mut audio_rx,
                    &events,
                    &mut stop_rx,
                )
                .await
            });
            let exchange = async {
                let mut speech_samples = 0;
                let mut silent_packets = 0;
                let mut silence_started = None;
                loop {
                    let message = server.next().await.unwrap().unwrap();
                    let value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                    if value.pointer("/realtimeInput/audioStreamEnd") == Some(&Value::Bool(true)) {
                        break;
                    }
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(
                            value
                                .pointer("/realtimeInput/audio/data")
                                .unwrap()
                                .as_str()
                                .unwrap(),
                        )
                        .unwrap();
                    assert_eq!(
                        value.pointer("/realtimeInput/audio/mimeType").unwrap(),
                        "audio/pcm;rate=16000"
                    );
                    if bytes.iter().all(|byte| *byte == 0) {
                        assert_eq!(speech_samples, 4900);
                        assert_eq!(bytes.len(), 3200);
                        silent_packets += 1;
                        silence_started.get_or_insert(tokio::time::Instant::now());
                        if silent_packets == 1 {
                            server
                                .send(json_frame(
                                    serde_json::json!({"serverContent": {
                                        "inputTranscription": {"text":"The train leaves at "},
                                        "outputTranscription": {"text":"火车"}
                                    }}),
                                    true,
                                ))
                                .await
                                .unwrap();
                        }
                    } else {
                        assert_eq!(silent_packets, 0);
                        speech_samples += bytes.len() / 2;
                    }
                }
                assert_eq!(silent_packets, 5);
                assert!(silence_started.unwrap().elapsed() >= Duration::from_millis(450));
                // Empty transcript events during silence are not sentence boundaries.
                server
                    .send(json_frame(
                        serde_json::json!({"serverContent": {
                            "inputTranscription": {"languageCode":"en"},
                            "outputTranscription": {"languageCode":"zh"}
                        }}),
                        true,
                    ))
                    .await
                    .unwrap();
                server
                    .send(json_frame(
                        serde_json::json!({"serverContent": {
                            "inputTranscription": {"text":"seven."},
                            "outputTranscription": {"text":"七点出发。"}
                        }}),
                        true,
                    ))
                    .await
                    .unwrap();
                server.close(None).await.unwrap();
                task.await.unwrap().unwrap();
                let mut originals = Vec::new();
                let mut translations = Vec::new();
                while let Some(CloudEvent::LiveTranslation {
                    completed,
                    translations: updates,
                    ..
                }) = received.recv().await
                {
                    originals.extend(completed);
                    translations.extend(updates.into_iter().filter(|update| !update.pending));
                }
                assert_eq!(originals.len(), 1);
                assert_eq!(originals[0].transcript.text, "The train leaves at seven.");
                assert_eq!(translations.len(), 1);
                assert_eq!(translations[0].transcript.translation, "火车七点出发。");
                assert_eq!(
                    originals[0].transcript.utterance_id,
                    translations[0].transcript.utterance_id
                );
            };
            tokio::time::timeout(Duration::from_secs(3), exchange)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn gemini_initializes_with_text_and_binary_confirmation() {
        for binary in [false, true] {
            let (mut client, mut server) = socket_pair().await;
            let config = AsrConfig::default();
            let update = Provider::Gemini.start_message(&config, 0.4, None).unwrap();
            let exchange = async {
                tokio::join!(
                    initialize_socket(Provider::Gemini, &mut client, update),
                    async {
                        assert!(matches!(server.next().await, Some(Ok(Message::Text(_)))));
                        server.send(Message::Ping(vec![1].into())).await.unwrap();
                        server
                            .send(json_frame(serde_json::json!({"setupComplete":{}}), binary))
                            .await
                            .unwrap();
                    }
                )
                .0
                .unwrap();
            };
            tokio::time::timeout(Duration::from_secs(2), exchange)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn gemini_streams_binary_transcripts_and_drains_the_last_sentence() {
        for binary in [false, true] {
            let (mut client, mut server) = socket_pair().await;
            let config = AsrConfig::default();
            let (audio_tx, mut audio_rx) = mpsc::channel(1);
            let (event_tx, mut event_rx) = mpsc::channel(4);
            let (stop_tx, mut stop_rx) = watch::channel(false);
            let (audio_seen_tx, audio_seen_rx) = oneshot::channel();
            let task = tokio::spawn(async move {
                run_session(
                    Provider::Gemini,
                    &config,
                    &mut client,
                    None,
                    &mut audio_rx,
                    &event_tx,
                    &mut stop_rx,
                )
                .await
            });
            let server_task = tokio::spawn(async move {
                server.send(json_frame(serde_json::json!({"serverContent":{"interimInputTranscription":{"text":"hello"}}}), binary)).await.unwrap();
                let audio = server.next().await.unwrap().unwrap();
                assert!(audio.to_text().unwrap().contains("audio/pcm"));
                audio_seen_tx.send(()).unwrap();
                let commit = server.next().await.unwrap().unwrap();
                assert!(commit.to_text().unwrap().contains("audioStreamEnd"));
                server.send(Message::Ping(vec![2].into())).await.unwrap();
                server.send(json_frame(serde_json::json!({"serverContent":{"inputTranscription":{"text":"hello world"}}}), binary)).await.unwrap();
                server.close(None).await.unwrap();
            });
            let exchange = async {
                let partial_id = match event_rx.recv().await.unwrap() {
                    CloudEvent::Partial {
                        utterance_id, text, ..
                    } => {
                        assert_eq!(text, "hello");
                        utterance_id
                    }
                    other => panic!("unexpected event: {other:?}"),
                };
                audio_tx
                    .send(StreamingInput::Audio(std::sync::Arc::new(
                        vec![0.0; AUDIO_PACKET_SAMPLES],
                    )))
                    .await
                    .unwrap();
                audio_seen_rx.await.unwrap();
                stop_tx.send(true).unwrap();
                task.await.unwrap().unwrap();
                assert!(
                    matches!(event_rx.recv().await, Some(CloudEvent::Final { utterance_id, text, .. })
                    if utterance_id == partial_id && text == "hello world")
                );
                server_task.await.unwrap();
            };
            tokio::time::timeout(Duration::from_secs(2), exchange)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn cloud_initialization_reports_binary_errors_and_close_reasons() {
        use tokio_tungstenite::tungstenite::{
            protocol::frame::coding::CloseCode, protocol::CloseFrame,
        };
        for (frame, expected) in [
            (
                json_frame(
                    serde_json::json!({"error":{"message":"Model unavailable"}}),
                    true,
                ),
                "Model unavailable",
            ),
            (
                Message::Close(Some(CloseFrame {
                    code: CloseCode::Policy,
                    reason: "Permission denied".into(),
                })),
                "1008 Permission denied",
            ),
            (Message::Binary(vec![0xff].into()), "invalid JSON"),
        ] {
            let (mut client, mut server) = socket_pair().await;
            let exchange = async {
                let (result, ()) = tokio::join!(
                    initialize_socket(
                        Provider::Gemini,
                        &mut client,
                        serde_json::json!({"setup":{}})
                    ),
                    async {
                        server.next().await.unwrap().unwrap();
                        server.send(frame).await.unwrap();
                    }
                );
                assert!(result.unwrap_err().contains(expected));
            };
            tokio::time::timeout(Duration::from_secs(2), exchange)
                .await
                .unwrap();
        }
    }

    fn normalize(
        config: &AsrConfig,
        message: &str,
        state: &mut NormalizationState,
    ) -> Result<Option<CloudEvent>, String> {
        normalize_event(Provider::from_config(config)?, config, message, state)
    }

    fn asr_profile(provider: &str) -> ApiProfile {
        ApiProfile {
            id: "profile".into(),
            name: "Test".into(),
            provider: provider.into(),
            enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
            ..ApiProfile::default()
        }
    }

    #[test]
    fn active_profile_must_own_the_selected_service() {
        let mut config = AsrConfig {
            backend: SERVICE_QWEN_REALTIME.into(),
            active_profile_id: Some("profile".into()),
            api_profiles: vec![asr_profile(ALIBABA_PROVIDER)],
            ..AsrConfig::default()
        };
        assert_eq!(active_profile(&config).unwrap().id, "profile");

        config.api_profiles[0].provider = OPENAI_PROVIDER.into();
        assert!(active_profile(&config).is_err());
    }

    #[test]
    fn explicit_test_service_selects_any_compatible_realtime_service() {
        let alibaba = asr_profile(ALIBABA_PROVIDER);
        assert_eq!(
            resolve_test_service(
                &alibaba,
                SERVICE_QWEN_REALTIME,
                Some(SERVICE_FUN_ASR_REALTIME)
            )
            .unwrap(),
            SERVICE_FUN_ASR_REALTIME
        );
        assert_eq!(
            resolve_test_service(
                &alibaba,
                SERVICE_FUN_ASR_REALTIME,
                Some(SERVICE_QWEN_REALTIME)
            )
            .unwrap(),
            SERVICE_QWEN_REALTIME
        );

        let openai = asr_profile(OPENAI_PROVIDER);
        assert_eq!(
            resolve_test_service(&openai, "local_whisper", Some(SERVICE_OPENAI_REALTIME)).unwrap(),
            SERVICE_OPENAI_REALTIME
        );
        assert!(resolve_test_service(
            &alibaba,
            SERVICE_QWEN_REALTIME,
            Some(SERVICE_OPENAI_REALTIME)
        )
        .is_err());
        assert!(resolve_test_service(
            &openai,
            SERVICE_OPENAI_REALTIME,
            Some(SERVICE_FUN_ASR_REALTIME)
        )
        .is_err());
        assert!(Provider::from_service(providers::SERVICE_GROQ_TRANSCRIPTION).is_err());
    }

    #[test]
    fn test_service_falls_back_to_a_realtime_service_on_the_profile() {
        let alibaba = asr_profile(ALIBABA_PROVIDER);
        assert_eq!(
            resolve_test_service(&alibaba, SERVICE_FUN_ASR_REALTIME, None).unwrap(),
            SERVICE_FUN_ASR_REALTIME
        );
        assert_eq!(
            resolve_test_service(&alibaba, SERVICE_OPENAI_REALTIME, None).unwrap(),
            SERVICE_QWEN_REALTIME
        );

        let openai = asr_profile(OPENAI_PROVIDER);
        assert_eq!(
            resolve_test_service(&openai, SERVICE_QWEN_REALTIME, None).unwrap(),
            SERVICE_OPENAI_REALTIME
        );
    }

    #[test]
    fn openai_deltas_are_accumulated_and_completed() {
        let config = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let first = normalize(&config, r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","delta":"hel"}"#, &mut state).unwrap();
        let second = normalize(&config, r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","delta":"lo"}"#, &mut state).unwrap();
        let final_event = normalize(&config, r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"a","transcript":"hello"}"#, &mut state).unwrap();
        assert!(matches!(first, Some(CloudEvent::Partial { text, .. }) if text == "hel"));
        assert!(matches!(second, Some(CloudEvent::Partial { text, .. }) if text == "hello"));
        assert!(matches!(final_event, Some(CloudEvent::Final { text, .. }) if text == "hello"));
        assert!(state.transcripts.is_empty());
    }

    #[test]
    fn protocol_event_ids_do_not_split_one_utterance() {
        let config = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let partial = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.delta","event_id":"event-1","delta":"hello"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();
        let final_event = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.completed","event_id":"event-2","transcript":"hello"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();

        let CloudEvent::Partial {
            utterance_id: partial_id,
            ..
        } = partial
        else {
            panic!("expected partial");
        };
        let CloudEvent::Final {
            utterance_id: final_id,
            ..
        } = final_event
        else {
            panic!("expected final");
        };
        assert_eq!(partial_id, final_id);
        assert_ne!(partial_id, "event-1");
        assert_ne!(final_id, "event-2");
    }

    #[test]
    fn snapshot_partial_id_is_reused_when_final_omits_it() {
        let config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let partial = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"item-1","text":"hello"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();
        let final_event = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.completed","event_id":"event-2","transcript":"hello"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();

        let CloudEvent::Partial {
            utterance_id: partial_id,
            ..
        } = partial
        else {
            panic!("expected partial");
        };
        let CloudEvent::Final {
            utterance_id: final_id,
            ..
        } = final_event
        else {
            panic!("expected final");
        };
        assert_eq!(partial_id, "item-1");
        assert_eq!(partial_id, final_id);
    }

    #[test]
    fn fallback_ids_rotate_after_final() {
        let config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let first = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.text","text":"first"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();
        let first_final = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.completed","transcript":"first"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();
        let second = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.text","text":"second"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();

        let CloudEvent::Partial {
            utterance_id: first_id,
            ..
        } = first
        else {
            panic!("expected first partial");
        };
        let CloudEvent::Final {
            utterance_id: final_id,
            ..
        } = first_final
        else {
            panic!("expected first final");
        };
        let CloudEvent::Partial {
            utterance_id: second_id,
            ..
        } = second
        else {
            panic!("expected second partial");
        };
        assert_eq!(first_id, final_id);
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn explicit_final_ids_take_priority_over_an_active_fallback() {
        let config = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.delta","delta":"first"}"#,
            &mut state,
        )
        .unwrap();
        normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"item-2","delta":"second"}"#,
            &mut state,
        )
        .unwrap();
        let final_event = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"item-2","transcript":"second"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();

        assert!(matches!(
            final_event,
            CloudEvent::Final { utterance_id, .. } if utterance_id == "item-2"
        ));
    }

    #[test]
    fn explicit_failure_ids_take_priority_over_an_active_fallback() {
        let config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.delta","delta":"first"}"#,
            &mut state,
        )
        .unwrap();
        let failure = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.failed","item_id":"item-2","message":"failed"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();

        assert!(matches!(
            failure,
            CloudEvent::Failed {
                utterance_id: Some(utterance_id),
                ..
            } if utterance_id == "item-2"
        ));
    }

    #[test]
    fn cloud_failures_keep_the_active_fallback_id() {
        let config = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let partial = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.delta","delta":"hello"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();
        let failure = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.failed","event_id":"failure-event","message":"failed"}"#,
            &mut state,
        )
        .unwrap()
        .unwrap();

        let CloudEvent::Partial { utterance_id, .. } = partial else {
            panic!("expected partial");
        };
        assert!(matches!(
            failure,
            CloudEvent::Failed {
                utterance_id: Some(failed_id),
                ..
            } if failed_id == utterance_id
        ));
    }

    #[test]
    fn cloud_transcripts_reject_too_many_active_ids() {
        let config = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        for index in 0..provider::MAX_ACTIVE_TRANSCRIPTS {
            let message = serde_json::json!({
                "type": "conversation.item.input_audio_transcription.delta",
                "item_id": format!("item-{index}"),
                "delta": "a",
            })
            .to_string();
            normalize(&config, &message, &mut state).unwrap();
        }
        let overflow = serde_json::json!({
            "type": "conversation.item.input_audio_transcription.delta",
            "item_id": "overflow",
            "delta": "a",
        })
        .to_string();
        assert_eq!(
            normalize(&config, &overflow, &mut state).unwrap_err(),
            "Cloud recognition exceeded the active transcript limit"
        );
    }

    #[test]
    fn cloud_transcripts_reject_oversized_delta() {
        let config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let message = serde_json::json!({
            "type": "conversation.item.input_audio_transcription.delta",
            "item_id": "a",
            "delta": "a".repeat(provider::MAX_TRANSCRIPT_BYTES + 1),
        })
        .to_string();
        assert_eq!(
            normalize(&config, &message, &mut state).unwrap_err(),
            "Cloud recognition transcript exceeded 65536 bytes"
        );
        assert!(!state.transcripts.contains_key("a"));
    }

    #[test]
    fn audio_buffer_flushes_the_final_partial_packet() {
        let mut buffer = Vec::new();
        let mut sent_samples = 0;
        for _ in 0..13 {
            buffer.extend(vec![0.0; 512]);
            while let Some(packet) = take_audio_packet(&mut buffer, AUDIO_PACKET_SAMPLES) {
                assert_eq!(packet.len(), 1600);
                sent_samples += packet.len();
            }
        }

        assert_eq!(sent_samples, 6400);
        assert_eq!(buffer.len(), 256);
        let final_packet = take_buffered_audio(&mut buffer).unwrap();
        assert_eq!(final_packet.len(), 256);
        assert!(buffer.is_empty());
        assert_eq!(sent_samples + final_packet.len(), 6656);
    }

    #[test]
    fn empty_audio_buffer_does_not_create_a_packet() {
        assert!(take_buffered_audio(&mut Vec::new()).is_none());
    }

    #[test]
    fn resampler_produces_24khz_length() {
        assert_eq!(resample_16k_to_24k(&vec![0.0; 1600]).len(), 2400);
    }

    #[test]
    fn qwen_partial_combines_stable_and_draft_text() {
        let config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let event = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"a","text":"hello ","stash":"world","language":"en"}"#,
            &mut state,
        )
        .unwrap();
        assert!(matches!(event, Some(CloudEvent::Partial { text, .. }) if text == "hello world"));
    }

    #[test]
    fn token_plan_transcription_delta_combines_text_and_stash() {
        let config = AsrConfig {
            backend: SERVICE_TOKEN_PLAN_REALTIME.into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let event = normalize(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","text":"hello ","stash":"world"}"#,
            &mut state,
        )
        .unwrap();
        assert!(matches!(event, Some(CloudEvent::Partial { text, .. }) if text == "hello world"));
    }

    fn message_json(message: Message) -> Value {
        let Message::Text(text) = message else {
            panic!("expected a text message");
        };
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn session_updates_use_local_commit_when_supported() {
        let mut qwen = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        qwen.language = "auto".into();
        qwen.service_settings
            .get_mut(SERVICE_QWEN_REALTIME)
            .unwrap()
            .context = "VRChat, VRCX".into();
        let qwen_update = session_update(&qwen, 0.7);
        assert_eq!(qwen_update["session"]["sample_rate"], 16_000);
        assert!(qwen_update["session"]["turn_detection"].is_null());
        assert!(qwen_update["session"]["input_audio_transcription"]
            .get("language")
            .is_none());
        assert_eq!(
            qwen_update["session"]["input_audio_transcription"]["corpus"]["text"],
            "VRChat, VRCX"
        );

        let mut openai = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        openai
            .service_settings
            .get_mut(SERVICE_OPENAI_REALTIME)
            .unwrap()
            .model = "gpt-custom-transcribe".into();
        let openai_update = session_update(&openai, 0.4);
        assert_eq!(
            openai_update["session"]["audio"]["input"]["format"]["rate"],
            24_000
        );
        assert_eq!(
            openai_update["session"]["audio"]["input"]["transcription"]["model"],
            "gpt-custom-transcribe"
        );
        assert!(openai_update["session"]["audio"]["input"]["turn_detection"].is_null());
    }

    #[test]
    fn fixed_language_is_forwarded_to_cloud_providers() {
        let qwen = AsrConfig {
            backend: "qwen_realtime".into(),
            language: "ja".into(),
            ..AsrConfig::default()
        };
        assert_eq!(
            session_update(&qwen, 0.7)["session"]["input_audio_transcription"]["language"],
            "ja"
        );

        let openai = AsrConfig {
            backend: "openai_realtime".into(),
            language: "ja".into(),
            ..AsrConfig::default()
        };
        assert_eq!(
            session_update(&openai, 0.4)["session"]["audio"]["input"]["transcription"]["language"],
            "ja"
        );

        let fun_asr = AsrConfig {
            backend: "fun_asr_realtime".into(),
            language: "ja".into(),
            ..AsrConfig::default()
        };
        assert_eq!(
            fun_run_task(&fun_asr, 0.4, "task-id")["payload"]["parameters"]["language_hints"][0],
            "ja"
        );
    }

    #[test]
    fn providers_expose_their_segmentation_capabilities() {
        assert_eq!(
            Provider::Qwen.segmentation_mode(),
            SegmentationMode::LocalCommit
        );
        assert_eq!(
            Provider::OpenAi.segmentation_mode(),
            SegmentationMode::LocalCommit
        );
        assert_eq!(
            Provider::TokenPlan.segmentation_mode(),
            SegmentationMode::LocalCommit
        );
        assert_eq!(
            Provider::FunAsr.segmentation_mode(),
            SegmentationMode::ServerVad
        );

        assert_eq!(
            message_json(Provider::Qwen.commit_message().unwrap())["type"],
            "input_audio_buffer.commit"
        );
        assert_eq!(
            message_json(Provider::OpenAi.commit_message().unwrap())["type"],
            "input_audio_buffer.commit"
        );
        assert!(Provider::FunAsr.commit_message().is_none());
    }

    #[test]
    fn qwen_request_uses_workspace_endpoint_and_realtime_headers() {
        let mut config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        config
            .service_settings
            .get_mut(SERVICE_QWEN_REALTIME)
            .unwrap()
            .model = "qwen-custom-realtime".into();
        let profile = ApiProfile {
            id: "profile".into(),
            name: "Test".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("china_beijing".into()),
            workspace_id: Some("ws-example".into()),
            base_url: None,
            ..ApiProfile::default()
        };
        let request = build_request(&config, &profile, "sk-test").unwrap();
        assert_eq!(request.uri().to_string(), "wss://ws-example.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model=qwen-custom-realtime");
        assert_eq!(request.headers()["Authorization"], "Bearer sk-test");
        assert_eq!(request.headers()["OpenAI-Beta"], "realtime=v1");
    }

    #[test]
    fn qwen_request_rejects_missing_workspace() {
        let config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        let profile = ApiProfile {
            id: "profile".into(),
            name: "Test".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("china_beijing".into()),
            workspace_id: Some(String::new()),
            base_url: None,
            ..ApiProfile::default()
        };
        assert_eq!(
            build_request(&config, &profile, "sk-test").unwrap_err(),
            "Alibaba Cloud Workspace ID is not configured"
        );
    }

    #[test]
    fn token_plan_request_uses_realtime_endpoint_without_workspace() {
        let config = AsrConfig {
            backend: SERVICE_TOKEN_PLAN_REALTIME.into(),
            ..AsrConfig::default()
        };
        let profile = ApiProfile {
            id: "token-plan".into(),
            name: "Token Plan".into(),
            provider: ALIBABA_TOKEN_PLAN_PROVIDER.into(),
            ..ApiProfile::default()
        };
        let request = build_request(&config, &profile, "sk-sp-test").unwrap();
        assert_eq!(
            request.uri().to_string(),
            "wss://token-plan.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model=qwen-audio-3.0-realtime-plus"
        );
        assert_eq!(request.headers()["Authorization"], "Bearer sk-sp-test");
        assert!(!request.headers().contains_key("OpenAI-Beta"));

        let update = session_update(&config, 0.4);
        assert_eq!(update["session"]["modalities"][0], "text");
        assert_eq!(update["session"]["input_audio_format"], "pcm");
        assert!(update["session"]["turn_detection"].is_null());
    }

    #[test]
    fn fun_asr_request_uses_inference_endpoint_without_realtime_header() {
        let config = AsrConfig {
            backend: "fun_asr_realtime".into(),
            ..AsrConfig::default()
        };
        let profile = ApiProfile {
            id: "profile".into(),
            name: "Test".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("singapore".into()),
            workspace_id: Some("ws-example".into()),
            base_url: None,
            ..ApiProfile::default()
        };
        let request = build_request(&config, &profile, "sk-test").unwrap();
        assert_eq!(
            request.uri().to_string(),
            "wss://ws-example.ap-southeast-1.maas.aliyuncs.com/api-ws/v1/inference"
        );
        assert_eq!(request.headers()["Authorization"], "Bearer sk-test");
        assert!(!request.headers().contains_key("OpenAI-Beta"));
    }

    #[test]
    fn fun_asr_run_task_contains_streaming_audio_and_context_options() {
        let mut config = AsrConfig {
            backend: "fun_asr_realtime".into(),
            language: "zh".into(),
            ..AsrConfig::default()
        };
        let settings = config
            .service_settings
            .get_mut(SERVICE_FUN_ASR_REALTIME)
            .unwrap();
        settings.model = "fun-asr-custom".into();
        settings.context = "VRChat 专有名词".into();
        let task = fun_run_task(&config, 0.7, "task-1");
        assert_eq!(task["header"]["action"], "run-task");
        assert_eq!(task["header"]["streaming"], "duplex");
        assert_eq!(task["payload"]["model"], "fun-asr-custom");
        assert_eq!(task["payload"]["parameters"]["format"], "pcm");
        assert_eq!(task["payload"]["parameters"]["sample_rate"], 16_000);
        assert_eq!(task["payload"]["parameters"]["max_sentence_silence"], 700);
        assert_eq!(task["payload"]["parameters"]["language_hints"][0], "zh");
        assert_eq!(
            task["payload"]["input"]["context"][0]["content"][0]["text"],
            "VRChat 专有名词"
        );
    }

    #[test]
    fn fun_asr_results_are_normalized_and_heartbeats_are_ignored() {
        let config = AsrConfig {
            backend: "fun_asr_realtime".into(),
            ..AsrConfig::default()
        };
        let mut state = NormalizationState::default();
        let partial = normalize(&config, r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"sentence_id":1,"text":"你好","sentence_end":false}}}}"#, &mut state).unwrap();
        let final_event = normalize(&config, r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"sentence_id":1,"text":"你好世界","sentence_end":true}}}}"#, &mut state).unwrap();
        let heartbeat = normalize(&config, r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"heartbeat":true,"text":""}}}}"#, &mut state).unwrap();
        let failure = normalize(&config, r#"{"header":{"event":"task-failed","error_code":"InvalidParameter","error_message":"bad request"}}"#, &mut state).unwrap();
        assert!(
            matches!(partial, Some(CloudEvent::Partial { utterance_id, text, .. }) if utterance_id == "1" && text == "你好")
        );
        assert!(matches!(final_event, Some(CloudEvent::Final { text, .. }) if text == "你好世界"));
        assert_eq!(heartbeat, None);
        assert!(
            matches!(failure, Some(CloudEvent::Failed { code, detail, .. }) if code == "InvalidParameter" && detail == "bad request")
        );
        assert_eq!(pcm16_bytes(&[0.0; 1600]).len(), 3200);
    }
}
