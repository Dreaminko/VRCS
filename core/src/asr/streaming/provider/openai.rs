use std::collections::HashMap;

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

use crate::config::AsrConfig;

use super::{
    authenticated_request, event_id, event_language, pcm16_base64, resample_16k_to_24k, CloudEvent,
};

pub(super) fn build_request(key: &str) -> Result<Request<()>, String> {
    authenticated_request(
        "wss://api.openai.com/v1/realtime?intent=transcription".into(),
        key,
        true,
    )
}

pub(super) fn session_update(config: &AsrConfig, silence_seconds: f64) -> Value {
    let mut transcription = json!({ "model": config.openai.model });
    if config.language != "auto" {
        transcription["language"] = json!(config.language);
    }
    json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": { "input": {
                "format": { "type": "audio/pcm", "rate": 24000 },
                "transcription": transcription,
                "turn_detection": {
                    "type": "server_vad",
                    "prefix_padding_ms": 200,
                    "silence_duration_ms": (silence_seconds * 1000.0).round() as u64,
                }
            }}
        }
    })
}

pub(super) fn normalize_event(
    config: &AsrConfig,
    value: &Value,
    transcripts: &mut HashMap<String, String>,
) -> Result<Option<CloudEvent>, String> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if kind == "error" || kind.ends_with(".failed") {
        let detail = value
            .pointer("/error/message")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("云端识别请求失败");
        return Ok(Some(CloudEvent::Failed {
            code: "asr.cloud_error".into(),
            detail: detail.into(),
        }));
    }

    let id = event_id(value);
    let language = event_language(config, value);
    if kind.ends_with(".delta") {
        let delta = value
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = transcripts.entry(id.clone()).or_default();
        text.push_str(delta);
        return Ok((!text.is_empty()).then(|| CloudEvent::Partial {
            utterance_id: id,
            text: text.clone(),
            language,
        }));
    }
    if kind.ends_with(".completed") {
        let text = value
            .get("transcript")
            .or_else(|| value.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| transcripts.remove(&id))
            .unwrap_or_default();
        transcripts.remove(&id);
        return Ok(Some(CloudEvent::Final {
            utterance_id: id,
            text,
            language,
        }));
    }
    if kind.ends_with(".text") {
        let text = format!(
            "{}{}",
            value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            value
                .get("stash")
                .and_then(Value::as_str)
                .unwrap_or_default()
        );
        return Ok((!text.is_empty()).then(|| CloudEvent::Partial {
            utterance_id: id,
            text,
            language,
        }));
    }
    Ok(None)
}

pub(super) fn audio_message(samples: &[f32]) -> Message {
    Message::Text(
        json!({
            "type": "input_audio_buffer.append",
            "audio": pcm16_base64(&resample_16k_to_24k(samples)),
        })
        .to_string()
        .into(),
    )
}

pub(super) fn finish_message() -> Message {
    Message::Text(
        json!({ "type": "input_audio_buffer.commit" })
            .to_string()
            .into(),
    )
}
