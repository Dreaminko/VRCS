use std::time::Duration;

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

mod provider;

pub use provider::SegmentationMode;
use provider::{InitializationEvent, NormalizationState, Provider};

#[cfg(test)]
use tokio_tungstenite::tungstenite::http::Request;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, PartialEq)]
pub enum CloudEvent {
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
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }

    pub async fn stop_and_drain(mut self) -> Vec<CloudEvent> {
        let _ = self.stop.send(true);
        let _ = self.task.await;
        let mut drained = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            drained.push(event);
        }
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
    )
    .await;
    Ok(())
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
    loop {
        let outcome = run_session(
            provider,
            &config,
            &mut socket,
            task_id.as_deref(),
            &mut audio,
            &events,
            &mut stop,
        )
        .await;
        if *stop.borrow() || audio.is_closed() {
            break;
        }
        let detail = match outcome {
            Ok(()) => "Cloud recognition connection was closed".to_string(),
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

async fn run_session(
    provider: Provider,
    config: &AsrConfig,
    socket: &mut Socket,
    task_id: Option<&str>,
    audio: &mut mpsc::Receiver<StreamingInput>,
    events: &mpsc::Sender<CloudEvent>,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let mut normalization = NormalizationState::default();
    let mut audio_buffer = Vec::with_capacity(2048);
    let mut pending_audio = false;
    loop {
        tokio::select! {
            _ = stop.changed() => {
                let _ = flush_audio_buffer(provider, socket, &mut audio_buffer).await;
                if pending_audio {
                    let _ = commit_utterance(provider, socket).await;
                }
                finish(provider, socket, config, task_id, &mut normalization, events).await;
                return Ok(());
            }
            input = audio.recv() => {
                match input {
                    Some(StreamingInput::Audio(samples)) => {
                        pending_audio = true;
                        audio_buffer.extend_from_slice(samples.as_slice());
                        while let Some(packet) = take_audio_packet(&mut audio_buffer) {
                            send_audio(provider, socket, packet).await?;
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
                        let _ = flush_audio_buffer(provider, socket, &mut audio_buffer).await;
                        if pending_audio {
                            let _ = commit_utterance(provider, socket).await;
                        }
                        finish(provider, socket, config, task_id, &mut normalization, events).await;
                        return Ok(());
                    }
                }
            }
            message = socket.next() => {
                let message = message
                    .ok_or_else(|| "Cloud recognition connection was closed".to_string())?
                    .map_err(|error| format!("Failed to read cloud recognition event: {error}"))?;
                match message {
                    Message::Text(text) => {
                        if let Some(event) = normalize_event(provider, config, &text, &mut normalization)? {
                            if events.send(event).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                    Message::Close(_) => return Err("Cloud recognition service closed the connection".into()),
                    Message::Ping(value) => socket.send(Message::Pong(value)).await
                        .map_err(|error| format!("Cloud recognition heartbeat failed: {error}"))?,
                    _ => {}
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
    socket
        .send(Message::Text(update.to_string().into()))
        .await
        .map_err(|error| format!("Failed to initialize cloud recognition session: {error}"))?;
    let configured = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or_else(|| {
                    "Cloud recognition connection closed during initialization".to_string()
                })?
                .map_err(|error| {
                    format!("Failed to read cloud recognition initialization event: {error}")
                })?;
            match message {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).map_err(|error| {
                        format!("Cloud recognition returned invalid JSON: {error}")
                    })?;
                    match provider.initialization_event(&value) {
                        InitializationEvent::Ready => return Ok(()),
                        InitializationEvent::Failed(detail) => return Err(detail),
                        InitializationEvent::Pending => {}
                    }
                }
                Message::Ping(value) => socket
                    .send(Message::Pong(value))
                    .await
                    .map_err(|error| format!("Cloud recognition heartbeat failed: {error}"))?,
                Message::Close(_) => {
                    return Err(
                        "Cloud recognition service closed the initialization connection".into(),
                    )
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| "Timed out waiting for cloud recognition session confirmation".to_string())?;
    configured?;
    Ok((socket, task_id))
}

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

const AUDIO_PACKET_SAMPLES: usize = 1600;

fn take_audio_packet(buffer: &mut Vec<f32>) -> Option<Vec<f32>> {
    (buffer.len() >= AUDIO_PACKET_SAMPLES).then(|| buffer.drain(..AUDIO_PACKET_SAMPLES).collect())
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
) {
    if let Some(message) = provider.finish_message(task_id) {
        let _ = socket.send(message).await;
    }
    let deadline = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            message = socket.next() => {
                let Some(Ok(Message::Text(text))) = message else { break };
                let finished = serde_json::from_str::<Value>(&text)
                    .ok()
                    .is_some_and(|value| provider.is_finished(&value));
                if let Ok(Some(event)) = normalize_event(provider, config, &text, state) {
                    let _ = events.send(event).await;
                }
                if finished {
                    break;
                }
            }
        }
    }
    let _ = socket.close(None).await;
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
            while let Some(packet) = take_audio_packet(&mut buffer) {
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
