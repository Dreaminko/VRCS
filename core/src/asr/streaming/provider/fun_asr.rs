use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

use crate::config::{ApiProfile, AsrConfig};

use super::{authenticated_request, pcm16_bytes, CloudEvent};

pub(super) fn build_request(profile: &ApiProfile, key: &str) -> Result<Request<()>, String> {
    let workspace = profile.workspace_id.as_deref().unwrap_or("").trim();
    if workspace.is_empty() {
        return Err("Alibaba Cloud Workspace ID is not configured".into());
    }
    let region = match profile.region.as_deref().unwrap_or("") {
        "singapore" => "ap-southeast-1",
        "china_beijing" => "cn-beijing",
        other => return Err(format!("Unsupported Alibaba Cloud region: {other}")),
    };
    authenticated_request(
        format!("wss://{workspace}.{region}.maas.aliyuncs.com/api-ws/v1/inference"),
        key,
        false,
    )
}

pub(super) fn run_task(config: &AsrConfig, silence_seconds: f64, task_id: &str) -> Value {
    let mut parameters = json!({
        "format": "pcm",
        "sample_rate": 16000,
        "semantic_punctuation_enabled": false,
        "max_sentence_silence": (silence_seconds * 1000.0).round() as u64,
    });
    if config.language != "auto" {
        parameters["language_hints"] = json!([config.language]);
    }
    let context = config.fun_asr.context.trim();
    let input = if context.is_empty() {
        json!({})
    } else {
        json!({
            "context": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": context }]
            }]
        })
    };
    json!({
        "header": {
            "action": "run-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": {
            "task_group": "audio",
            "task": "asr",
            "function": "recognition",
            "model": config.fun_asr.model,
            "parameters": parameters,
            "input": input
        }
    })
}

pub(super) fn normalize_event(
    config: &AsrConfig,
    value: &Value,
) -> Result<Option<CloudEvent>, String> {
    let kind = value
        .pointer("/header/event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if kind == "task-failed" {
        return Ok(Some(CloudEvent::Failed {
            code: value
                .pointer("/header/error_code")
                .and_then(Value::as_str)
                .unwrap_or("asr.cloud_error")
                .to_string(),
            detail: value
                .pointer("/header/error_message")
                .and_then(Value::as_str)
                .unwrap_or("Fun-ASR request failed")
                .to_string(),
        }));
    }
    if kind != "result-generated" {
        return Ok(None);
    }
    let Some(sentence) = value.pointer("/payload/output/sentence") else {
        return Ok(None);
    };
    if sentence
        .get("heartbeat")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let text = sentence
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if text.is_empty() {
        return Ok(None);
    }
    let utterance_id = sentence
        .get("sentence_id")
        .map(|id| {
            id.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| id.to_string())
        })
        .unwrap_or_else(|| "current".into());
    let language = (config.language != "auto").then(|| config.language.clone());
    if sentence
        .get("sentence_end")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Ok(Some(CloudEvent::Final {
            utterance_id,
            text,
            language,
        }))
    } else {
        Ok(Some(CloudEvent::Partial {
            utterance_id,
            text,
            language,
        }))
    }
}

pub(super) fn audio_message(samples: &[f32]) -> Message {
    Message::Binary(pcm16_bytes(samples).into())
}

pub(super) fn finish_message(task_id: &str) -> Message {
    Message::Text(
        json!({
            "header": { "action": "finish-task", "task_id": task_id },
            "payload": { "input": {} }
        })
        .to_string()
        .into(),
    )
}
