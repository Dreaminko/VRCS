mod fun_asr;
mod gemini;
mod openai;
mod qwen;

use std::collections::HashMap;

use serde_json::Value;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

use crate::config::{ApiProfile, AsrConfig, RecognitionServiceSettings};
use crate::providers::{self, RecognitionTransport, ServiceAdapter};

use super::CloudEvent;

pub(super) const MAX_ACTIVE_TRANSCRIPTS: usize = 32;
pub(super) const MAX_TRANSCRIPT_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct NormalizationState {
    pub(super) transcripts: HashMap<String, String>,
    fallback_id: Option<String>,
    snapshot_id: Option<String>,
}

impl NormalizationState {
    fn delta_id(&mut self, value: &Value) -> String {
        explicit_utterance_id(value).unwrap_or_else(|| self.fallback_id())
    }

    fn snapshot_id(&mut self, value: &Value) -> String {
        if let Some(id) = explicit_utterance_id(value) {
            self.snapshot_id = Some(id.clone());
            return id;
        }
        if let Some(id) = &self.snapshot_id {
            return id.clone();
        }
        self.fallback_id()
    }

    fn remember_snapshot(&mut self, id: &str) {
        self.snapshot_id = Some(id.to_owned());
    }

    fn final_id(&mut self, value: &Value) -> String {
        if let Some(id) = explicit_utterance_id(value) {
            return id;
        }
        if let Some(id) = &self.fallback_id {
            return id.clone();
        }
        if let Some(id) = &self.snapshot_id {
            return id.clone();
        }
        if self.transcripts.len() == 1 {
            return self
                .transcripts
                .keys()
                .next()
                .cloned()
                .expect("one transcript");
        }
        self.fallback_id()
    }

    fn fail(&mut self, value: &Value) -> Option<String> {
        let id = explicit_utterance_id(value)
            .or_else(|| self.fallback_id.clone())
            .or_else(|| self.snapshot_id.clone())
            .or_else(|| {
                (self.transcripts.len() == 1)
                    .then(|| self.transcripts.keys().next().cloned())
                    .flatten()
            });
        if let Some(id) = &id {
            self.complete(id);
        } else {
            self.transcripts.clear();
            self.fallback_id = None;
            self.snapshot_id = None;
        }
        id
    }

    fn fallback_id(&mut self) -> String {
        if let Some(id) = &self.fallback_id {
            return id.clone();
        }
        let id = format!("local-utterance-{}", uuid::Uuid::new_v4());
        self.fallback_id = Some(id.clone());
        id
    }

    fn append_transcript(&mut self, id: &str, delta: &str) -> Result<&str, String> {
        append_transcript(&mut self.transcripts, id, delta)
    }

    fn take_transcript(&mut self, id: &str) -> Option<String> {
        self.transcripts.remove(id)
    }

    fn complete(&mut self, id: &str) {
        self.transcripts.remove(id);
        if self.fallback_id.as_deref() == Some(id) {
            self.fallback_id = None;
        }
        if self.snapshot_id.as_deref() == Some(id) {
            self.snapshot_id = None;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Provider {
    Qwen,
    TokenPlan,
    FunAsr,
    OpenAi,
    Gemini,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentationMode {
    LocalCommit,
    ServerVad,
}

pub(super) enum InitializationEvent {
    Ready,
    Failed(String),
    Pending,
}

impl Provider {
    pub(super) fn from_config(config: &AsrConfig) -> Result<Self, String> {
        Self::from_service(&config.backend)
    }

    pub(super) fn from_service(service_id: &str) -> Result<Self, String> {
        let (_, service) = providers::recognition_service(service_id)
            .ok_or_else(|| format!("Service {service_id} is not a recognition service"))?;
        if service.recognition_transport != Some(RecognitionTransport::RealtimeStream) {
            return Err(format!(
                "Service {service_id} is not a realtime cloud recognition service"
            ));
        }
        match service.adapter {
            ServiceAdapter::QwenRealtime => Ok(Self::Qwen),
            ServiceAdapter::AlibabaTokenPlanRealtime => Ok(Self::TokenPlan),
            ServiceAdapter::FunAsrRealtime => Ok(Self::FunAsr),
            ServiceAdapter::OpenAiRealtime => Ok(Self::OpenAi),
            ServiceAdapter::GeminiTranscribe => Ok(Self::Gemini),
            _ => Err(format!(
                "Service {service_id} does not have a realtime recognition adapter"
            )),
        }
    }

    pub(super) fn build_request(
        self,
        config: &AsrConfig,
        profile: &ApiProfile,
        key: &str,
    ) -> Result<Request<()>, String> {
        match self {
            Self::Qwen => qwen::build_request(config, profile, key),
            Self::TokenPlan => qwen::build_token_plan_request(config, key),
            Self::FunAsr => fun_asr::build_request(profile, key),
            Self::OpenAi => openai::build_request(key),
            Self::Gemini => gemini::build_request(key),
        }
    }

    pub(super) fn task_id(self) -> Option<String> {
        (self == Self::FunAsr).then(|| uuid::Uuid::new_v4().to_string())
    }

    pub(super) fn segmentation_mode(self) -> SegmentationMode {
        match self {
            Self::Qwen | Self::TokenPlan | Self::OpenAi | Self::Gemini => {
                SegmentationMode::LocalCommit
            }
            Self::FunAsr => SegmentationMode::ServerVad,
        }
    }

    pub(super) fn start_message(
        self,
        config: &AsrConfig,
        silence_seconds: f64,
        task_id: Option<&str>,
    ) -> Result<Value, String> {
        match self {
            Self::Qwen => qwen::session_update(config),
            Self::TokenPlan => Ok(qwen::token_plan_session_update()),
            Self::FunAsr => fun_asr::run_task(
                config,
                silence_seconds,
                task_id.expect("Fun-ASR sessions always have a task id"),
            ),
            Self::OpenAi => openai::session_update(config),
            Self::Gemini => gemini::setup(config),
        }
    }

    pub(super) fn initialization_event(self, value: &Value) -> InitializationEvent {
        match self {
            Self::Qwen | Self::TokenPlan | Self::OpenAi => {
                match value.get("type").and_then(Value::as_str) {
                    Some("session.updated") => InitializationEvent::Ready,
                    Some("error") => InitializationEvent::Failed(
                        value
                            .pointer("/error/message")
                            .and_then(Value::as_str)
                            .unwrap_or("Cloud recognition session configuration failed")
                            .to_string(),
                    ),
                    _ => InitializationEvent::Pending,
                }
            }
            Self::FunAsr => match value.pointer("/header/event").and_then(Value::as_str) {
                Some("task-started") => InitializationEvent::Ready,
                Some("task-failed") => InitializationEvent::Failed(
                    value
                        .pointer("/header/error_message")
                        .and_then(Value::as_str)
                        .unwrap_or("Cloud recognition session configuration failed")
                        .to_string(),
                ),
                _ => InitializationEvent::Pending,
            },
            Self::Gemini => gemini::initialization_event(value),
        }
    }

    pub(super) fn normalize_event(
        self,
        config: &AsrConfig,
        value: &Value,
        state: &mut NormalizationState,
    ) -> Result<Option<CloudEvent>, String> {
        match self {
            Self::Qwen | Self::TokenPlan => qwen::normalize_event(config, value, state),
            Self::FunAsr => fun_asr::normalize_event(config, value, state),
            Self::OpenAi => openai::normalize_event(config, value, state),
            Self::Gemini => gemini::normalize_event(config, value, state),
        }
    }

    pub(super) fn audio_message(self, samples: &[f32]) -> Message {
        match self {
            Self::Qwen | Self::TokenPlan => qwen::audio_message(samples),
            Self::FunAsr => fun_asr::audio_message(samples),
            Self::OpenAi => openai::audio_message(samples),
            Self::Gemini => gemini::audio_message(samples),
        }
    }

    pub(super) fn commit_message(self) -> Option<Message> {
        match self {
            Self::Qwen | Self::TokenPlan => Some(qwen::commit_message()),
            Self::OpenAi => Some(openai::commit_message()),
            Self::Gemini => Some(gemini::commit_message()),
            Self::FunAsr => None,
        }
    }

    pub(super) fn finish_message(self, task_id: Option<&str>) -> Option<Message> {
        match self {
            Self::Qwen => Some(qwen::finish_message()),
            Self::TokenPlan => None,
            Self::FunAsr => Some(fun_asr::finish_message(
                task_id.expect("Fun-ASR sessions always have a task id"),
            )),
            Self::OpenAi => None,
            Self::Gemini => None,
        }
    }

    pub(super) fn is_finished(self, value: &Value) -> bool {
        match self {
            Self::Qwen => value.get("type").and_then(Value::as_str) == Some("session.finished"),
            Self::TokenPlan => false,
            Self::FunAsr => {
                value.pointer("/header/event").and_then(Value::as_str) == Some("task-finished")
            }
            Self::OpenAi => false,
            Self::Gemini => false,
        }
    }

    pub(super) fn connection_error(self, error: impl std::fmt::Display) -> String {
        if self == Self::Gemini {
            "Failed to connect to Gemini transcription service".into()
        } else {
            format!("Failed to connect to cloud recognition service: {error}")
        }
    }
}

pub(super) fn service_settings<'a>(
    config: &'a AsrConfig,
    service_id: &str,
) -> Result<&'a RecognitionServiceSettings, String> {
    config
        .service_settings
        .get(service_id)
        .ok_or_else(|| format!("Recognition settings are missing for service {service_id}"))
}

pub(super) fn pcm16_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    bytes
}

pub(super) fn pcm16_base64(samples: &[f32]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(pcm16_bytes(samples))
}

pub(super) fn resample_16k_to_24k(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let output_len = samples.len() * 3 / 2;
    (0..output_len)
        .map(|index| {
            let position = index as f32 * 2.0 / 3.0;
            let left = position.floor() as usize;
            let fraction = position - left as f32;
            let right = (left + 1).min(samples.len() - 1);
            samples[left] + (samples[right] - samples[left]) * fraction
        })
        .collect()
}

pub(super) fn authenticated_request(
    url: String,
    key: &str,
    realtime_header: bool,
) -> Result<Request<()>, String> {
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("Invalid cloud recognition URL: {error}"))?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| "API key contains invalid characters".to_string())?,
    );
    if realtime_header {
        request
            .headers_mut()
            .insert("OpenAI-Beta", HeaderValue::from_static("realtime=v1"));
    }
    Ok(request)
}

pub(super) fn append_transcript<'a>(
    transcripts: &'a mut HashMap<String, String>,
    id: &str,
    delta: &str,
) -> Result<&'a str, String> {
    if delta.is_empty() {
        return Ok(transcripts.get(id).map(String::as_str).unwrap_or_default());
    }
    let current_len = transcripts.get(id).map_or(0, String::len);
    if current_len.saturating_add(delta.len()) > MAX_TRANSCRIPT_BYTES {
        return Err("Cloud recognition transcript exceeded 65536 bytes".into());
    }
    if !transcripts.contains_key(id) && transcripts.len() >= MAX_ACTIVE_TRANSCRIPTS {
        return Err("Cloud recognition exceeded the active transcript limit".into());
    }
    let text = transcripts.entry(id.to_owned()).or_default();
    text.push_str(delta);
    Ok(text)
}

fn explicit_utterance_id(value: &Value) -> Option<String> {
    value
        .get("item_id")
        .or_else(|| value.get("utterance_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

pub(super) fn event_language(config: &AsrConfig, value: &Value) -> Option<String> {
    value
        .get("language")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| (config.language != "auto").then(|| config.language.clone()))
}
