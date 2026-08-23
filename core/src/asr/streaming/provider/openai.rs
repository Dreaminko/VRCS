use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

use crate::config::AsrConfig;
use crate::providers::SERVICE_OPENAI_REALTIME;

use super::{
    authenticated_request, event_language, pcm16_base64, resample_16k_to_24k, service_settings,
    CloudEvent, NormalizationState,
};

const REALTIME_SESSION_URL: &str = "wss://api.openai.com/v1/realtime?model=gpt-realtime";

pub(super) fn build_request(key: &str) -> Result<Request<()>, String> {
    authenticated_request(REALTIME_SESSION_URL.into(), key, false)
}

pub(super) fn session_update(config: &AsrConfig) -> Result<Value, String> {
    let settings = service_settings(config, SERVICE_OPENAI_REALTIME)?;
    let mut transcription = json!({ "model": settings.model });
    if config.language != "auto" {
        transcription["language"] = json!(config.language);
    }
    Ok(json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": { "input": {
                "format": { "type": "audio/pcm", "rate": 24000 },
                "transcription": transcription,
                "turn_detection": null
            }}
        }
    }))
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
            "audio": pcm16_base64(&resample_16k_to_24k(samples)),
        })
        .to_string()
        .into(),
    )
}

pub(super) fn commit_message() -> Message {
    Message::Text(
        json!({ "type": "input_audio_buffer.commit" })
            .to_string()
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_a_realtime_session_model() {
        let request = build_request("test-key").unwrap();

        assert_eq!(
            request.uri(),
            "wss://api.openai.com/v1/realtime?model=gpt-realtime"
        );
        assert!(request.headers().get("OpenAI-Beta").is_none());
        assert_eq!(
            request.headers().get("Authorization").unwrap(),
            "Bearer test-key"
        );
    }

    #[test]
    fn configured_model_is_only_used_for_input_transcription() {
        let mut config = AsrConfig::default();
        let default_update = session_update(&config).unwrap();
        assert_eq!(
            default_update["session"]["audio"]["input"]["transcription"]["model"],
            "gpt-4o-mini-transcribe"
        );

        config
            .service_settings
            .get_mut(SERVICE_OPENAI_REALTIME)
            .unwrap()
            .model = "gpt-4o-transcribe".into();
        let configured_update = session_update(&config).unwrap();
        assert_eq!(
            configured_update["session"]["audio"]["input"]["transcription"]["model"],
            "gpt-4o-transcribe"
        );
    }
}
