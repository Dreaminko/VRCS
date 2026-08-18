use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

use crate::config::{ApiProfile, AsrConfig};

use super::{authenticated_request, event_language, pcm16_base64, CloudEvent, NormalizationState};

pub(super) fn build_request(
    config: &AsrConfig,
    profile: &ApiProfile,
    key: &str,
) -> Result<Request<()>, String> {
    let workspace = profile.workspace_id.as_deref().unwrap_or("").trim();
    if workspace.is_empty() {
        return Err("Alibaba Cloud Workspace ID is not configured".into());
    }
    let region = match profile.region.as_deref().unwrap_or("") {
        "singapore" => "ap-southeast-1",
        "china_beijing" => "cn-beijing",
        other => return Err(format!("Unsupported Alibaba Cloud region: {other}")),
    };
    let url = format!(
        "wss://{}.{}.maas.aliyuncs.com/api-ws/v1/realtime?model={}",
        workspace, region, config.qwen.model
    );
    authenticated_request(url, key, true)
}

pub(super) fn session_update(config: &AsrConfig) -> Value {
    let mut transcription = if config.language != "auto" {
        json!({ "language": config.language })
    } else {
        json!({})
    };
    if !config.qwen.context.trim().is_empty() {
        transcription["corpus"] = json!({ "text": config.qwen.context.trim() });
    }
    json!({
        "event_id": uuid::Uuid::new_v4().to_string(),
        "type": "session.update",
        "session": {
            "input_audio_format": "pcm",
            "sample_rate": 16000,
            "input_audio_transcription": transcription,
            "turn_detection": null
        }
    })
}

pub(super) fn normalize_event(
    config: &AsrConfig,
    value: &Value,
    state: &mut NormalizationState,
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
            .unwrap_or("Cloud recognition request failed");
        let utterance_id = state.fail(value);
        return Ok(Some(CloudEvent::Failed {
            reset_session: utterance_id.is_none(),
            utterance_id,
            code: "asr.cloud_error".into(),
            detail: detail.into(),
        }));
    }

    let language = event_language(config, value);
    if kind.ends_with(".delta") {
        let id = state.delta_id(value);
        let delta = value
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = state.append_transcript(&id, delta)?;
        return Ok((!text.is_empty()).then(|| CloudEvent::Partial {
            utterance_id: id,
            text: text.to_owned(),
            language,
        }));
    }
    if kind.ends_with(".completed") {
        let id = state.final_id(value);
        let text = value
            .get("transcript")
            .or_else(|| value.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| state.take_transcript(&id))
            .unwrap_or_default();
        state.complete(&id);
        return Ok(Some(CloudEvent::Final {
            utterance_id: id,
            text,
            language,
        }));
    }
    if kind.ends_with(".text") {
        let id = state.snapshot_id(value);
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
        return Ok((!text.is_empty()).then_some(CloudEvent::Partial {
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
            "audio": pcm16_base64(samples),
            "event_id": uuid::Uuid::new_v4().to_string(),
        })
        .to_string()
        .into(),
    )
}

pub(super) fn commit_message() -> Message {
    Message::Text(
        json!({
            "event_id": uuid::Uuid::new_v4().to_string(),
            "type": "input_audio_buffer.commit"
        })
        .to_string()
        .into(),
    )
}

pub(super) fn finish_message() -> Message {
    Message::Text(
        json!({
            "event_id": uuid::Uuid::new_v4().to_string(),
            "type": "session.finish"
        })
        .to_string()
        .into(),
    )
}
