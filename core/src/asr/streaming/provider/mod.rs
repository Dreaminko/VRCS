mod fun_asr;
mod openai;
mod qwen;

use std::collections::HashMap;

use serde_json::Value;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

use crate::config::{ApiProfile, AsrConfig};
use crate::providers::{ALIBABA_PROVIDER, OPENAI_PROVIDER};

use super::CloudEvent;

pub(super) const MAX_ACTIVE_TRANSCRIPTS: usize = 32;
pub(super) const MAX_TRANSCRIPT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Provider {
    Qwen,
    FunAsr,
    OpenAi,
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
        match config.backend.as_str() {
            "qwen_realtime" => Ok(Self::Qwen),
            "fun_asr_realtime" => Ok(Self::FunAsr),
            "openai_realtime" => Ok(Self::OpenAi),
            other => Err(format!(
                "Backend {other} is not a realtime cloud recognition backend"
            )),
        }
    }

    pub(super) fn api_provider(self) -> &'static str {
        match self {
            Self::Qwen | Self::FunAsr => ALIBABA_PROVIDER,
            Self::OpenAi => OPENAI_PROVIDER,
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
            Self::FunAsr => fun_asr::build_request(profile, key),
            Self::OpenAi => openai::build_request(key),
        }
    }

    pub(super) fn task_id(self) -> Option<String> {
        (self == Self::FunAsr).then(|| uuid::Uuid::new_v4().to_string())
    }

    pub(super) fn segmentation_mode(self) -> SegmentationMode {
        match self {
            Self::Qwen | Self::OpenAi => SegmentationMode::LocalCommit,
            Self::FunAsr => SegmentationMode::ServerVad,
        }
    }

    pub(super) fn start_message(
        self,
        config: &AsrConfig,
        silence_seconds: f64,
        task_id: Option<&str>,
    ) -> Value {
        match self {
            Self::Qwen => qwen::session_update(config),
            Self::FunAsr => fun_asr::run_task(
                config,
                silence_seconds,
                task_id.expect("Fun-ASR sessions always have a task id"),
            ),
            Self::OpenAi => openai::session_update(config),
        }
    }

    pub(super) fn initialization_event(self, value: &Value) -> InitializationEvent {
        match self {
            Self::Qwen | Self::OpenAi => match value.get("type").and_then(Value::as_str) {
                Some("session.updated") => InitializationEvent::Ready,
                Some("error") => InitializationEvent::Failed(
                    value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Cloud recognition session configuration failed")
                        .to_string(),
                ),
                _ => InitializationEvent::Pending,
            },
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
        }
    }

    pub(super) fn normalize_event(
        self,
        config: &AsrConfig,
        value: &Value,
        transcripts: &mut HashMap<String, String>,
    ) -> Result<Option<CloudEvent>, String> {
        match self {
            Self::Qwen => qwen::normalize_event(config, value, transcripts),
            Self::FunAsr => fun_asr::normalize_event(config, value),
            Self::OpenAi => openai::normalize_event(config, value, transcripts),
        }
    }

    pub(super) fn audio_message(self, samples: &[f32]) -> Message {
        match self {
            Self::Qwen => qwen::audio_message(samples),
            Self::FunAsr => fun_asr::audio_message(samples),
            Self::OpenAi => openai::audio_message(samples),
        }
    }

    pub(super) fn commit_message(self) -> Option<Message> {
        match self {
            Self::Qwen => Some(qwen::commit_message()),
            Self::OpenAi => Some(openai::commit_message()),
            Self::FunAsr => None,
        }
    }

    pub(super) fn finish_message(self, task_id: Option<&str>) -> Option<Message> {
        match self {
            Self::Qwen => Some(qwen::finish_message()),
            Self::FunAsr => Some(fun_asr::finish_message(
                task_id.expect("Fun-ASR sessions always have a task id"),
            )),
            Self::OpenAi => None,
        }
    }

    pub(super) fn is_finished(self, value: &Value) -> bool {
        match self {
            Self::Qwen => value.get("type").and_then(Value::as_str) == Some("session.finished"),
            Self::FunAsr => {
                value.pointer("/header/event").and_then(Value::as_str) == Some("task-finished")
            }
            Self::OpenAi => false,
        }
    }
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

pub(super) fn event_id(value: &Value) -> String {
    value
        .get("item_id")
        .or_else(|| value.get("utterance_id"))
        .or_else(|| value.get("event_id"))
        .and_then(Value::as_str)
        .unwrap_or("current")
        .to_string()
}

pub(super) fn event_language(config: &AsrConfig, value: &Value) -> Option<String> {
    value
        .get("language")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| (config.language != "auto").then(|| config.language.clone()))
}
