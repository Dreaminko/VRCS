use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

use crate::config::AsrConfig;
use crate::providers::SERVICE_GEMINI_TRANSCRIBE;

use super::{
    event_language, pcm16_base64, service_settings, CloudEvent, InitializationEvent,
    NormalizationState,
};

const LIVE_API_URL: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

pub(super) fn build_request(key: &str) -> Result<Request<()>, String> {
    let mut url = reqwest::Url::parse(LIVE_API_URL)
        .map_err(|error| format!("Invalid Gemini transcription URL: {error}"))?;
    url.query_pairs_mut().append_pair("key", key);
    url.as_str()
        .into_client_request()
        .map_err(|error| format!("Invalid Gemini transcription request: {error}"))
}

pub(super) fn setup(config: &AsrConfig) -> Result<Value, String> {
    let settings = service_settings(config, SERVICE_GEMINI_TRANSCRIBE)?;
    let language_codes: Vec<&str> = language_code(&config.language).into_iter().collect();
    let vocabulary = custom_vocabulary(&settings.context);
    let mut transcription = json!({ "languageCodes": language_codes });
    if !vocabulary.is_empty() {
        transcription["customVocabulary"] = json!(vocabulary);
    }

    Ok(json!({
        "setup": {
            "model": format!("models/{}", settings.model),
            "generationConfig": { "responseModalities": ["TEXT"] },
            "inputAudioTranscription": transcription
        }
    }))
}

pub(super) fn initialization_event(value: &Value) -> InitializationEvent {
    if value.get("setupComplete").is_some() {
        return InitializationEvent::Ready;
    }
    if let Some(error) = value.get("error") {
        return InitializationEvent::Failed(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Gemini transcription session configuration failed")
                .to_string(),
        );
    }
    InitializationEvent::Pending
}

pub(super) fn normalize_event(
    config: &AsrConfig,
    value: &Value,
    state: &mut NormalizationState,
) -> Result<Option<CloudEvent>, String> {
    if let Some(error) = value.get("error") {
        let utterance_id = state.fail(value);
        return Ok(Some(CloudEvent::Failed {
            reset_session: utterance_id.is_none(),
            utterance_id,
            code: "asr.cloud_error".into(),
            detail: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Gemini transcription request failed")
                .into(),
        }));
    }

    let language = event_language(config, value);
    if let Some(text) = value
        .pointer("/serverContent/inputTranscription/text")
        .and_then(Value::as_str)
    {
        let id = state.final_id(value);
        state.complete(&id);
        return Ok(Some(CloudEvent::Final {
            utterance_id: id,
            text: text.to_owned(),
            language,
        }));
    }
    if let Some(text) = value
        .pointer("/serverContent/interimInputTranscription/text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        return Ok(Some(CloudEvent::Partial {
            utterance_id: state.snapshot_id(value),
            text: text.to_owned(),
            language,
        }));
    }
    Ok(None)
}

pub(super) fn audio_message(samples: &[f32]) -> Message {
    Message::Text(
        json!({
            "realtimeInput": {
                "audio": {
                    "data": pcm16_base64(samples),
                    "mimeType": "audio/pcm;rate=16000"
                }
            }
        })
        .to_string()
        .into(),
    )
}

pub(super) fn commit_message() -> Message {
    Message::Text(
        json!({ "realtimeInput": { "audioStreamEnd": true } })
            .to_string()
            .into(),
    )
}

fn language_code(language: &str) -> Option<&str> {
    Some(match language {
        "auto" => return None,
        "en" => "en-US",
        "ja" => "ja-JP",
        "zh" => "cmn-Hans-CN",
        "ko" => "ko-KR",
        "es" => "es-419",
        "fr" => "fr-FR",
        "de" => "de-DE",
        other => other,
    })
}

fn custom_vocabulary(context: &str) -> Vec<&str> {
    let mut terms = Vec::new();
    for term in context
        .lines()
        .map(str::trim)
        .filter(|term| !term.is_empty())
    {
        if !terms.contains(&term) {
            terms.push(term);
        }
        if terms.len() == 100 {
            break;
        }
    }
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_the_documented_websocket_endpoint() {
        let request = build_request("test-key").unwrap();
        assert_eq!(
            request.uri().to_string(),
            format!("{LIVE_API_URL}?key=test-key")
        );
    }

    #[test]
    fn setup_maps_language_and_context_to_transcription_config() {
        let mut config = AsrConfig::default();
        config.language = "zh".into();
        config
            .service_settings
            .get_mut(SERVICE_GEMINI_TRANSCRIBE)
            .unwrap()
            .context = "VRChat\n Gemini \nVRChat\n".into();

        let message = setup(&config).unwrap();
        assert_eq!(
            message.pointer("/setup/model").and_then(Value::as_str),
            Some("models/gemini-3.5-transcribe-live")
        );
        assert_eq!(
            message.pointer("/setup/inputAudioTranscription/languageCodes"),
            Some(&json!(["cmn-Hans-CN"]))
        );
        assert_eq!(
            message.pointer("/setup/inputAudioTranscription/customVocabulary"),
            Some(&json!(["VRChat", "Gemini"]))
        );
    }

    #[test]
    fn setup_uses_empty_language_codes_for_auto_detection() {
        let message = setup(&AsrConfig::default()).unwrap();
        assert_eq!(
            message.pointer("/setup/inputAudioTranscription/languageCodes"),
            Some(&json!([]))
        );
    }

    #[test]
    fn normalizes_interim_and_final_transcripts() {
        let config = AsrConfig::default();
        let mut state = NormalizationState::default();
        let partial = normalize_event(
            &config,
            &json!({"serverContent":{"interimInputTranscription":{"text":"hello"}}}),
            &mut state,
        )
        .unwrap()
        .unwrap();
        let partial_id = match partial {
            CloudEvent::Partial {
                utterance_id,
                text,
                language,
            } => {
                assert_eq!(text, "hello");
                assert_eq!(language, None);
                utterance_id
            }
            other => panic!("unexpected event: {other:?}"),
        };

        let final_event = normalize_event(
            &config,
            &json!({"serverContent":{"inputTranscription":{"text":"hello world"}}}),
            &mut state,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            final_event,
            CloudEvent::Final {
                utterance_id: partial_id,
                text: "hello world".into(),
                language: None,
            }
        );
    }

    #[test]
    fn audio_and_commit_messages_follow_the_live_api_shape() {
        let audio = match audio_message(&[0.0]) {
            Message::Text(text) => serde_json::from_str::<Value>(&text).unwrap(),
            other => panic!("unexpected message: {other:?}"),
        };
        assert_eq!(
            audio.pointer("/realtimeInput/audio/mimeType"),
            Some(&json!("audio/pcm;rate=16000"))
        );
        assert_eq!(
            audio.pointer("/realtimeInput/audio/data"),
            Some(&json!("AAA="))
        );

        let commit = match commit_message() {
            Message::Text(text) => serde_json::from_str::<Value>(&text).unwrap(),
            other => panic!("unexpected message: {other:?}"),
        };
        assert_eq!(
            commit.pointer("/realtimeInput/audioStreamEnd"),
            Some(&json!(true))
        );
    }
}
