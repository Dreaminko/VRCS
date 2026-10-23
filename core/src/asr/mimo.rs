use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

use crate::config::{ApiProfile, RecognitionServiceSettings};
use crate::providers;

use super::openai_audio_transcriptions::{encode_wav, network_error, status_error};
use super::transcription::Transcription;

const MAX_AUDIO_DATA_BYTES: usize = 10_000_000;
const AUDIO_DATA_PREFIX: &str = "data:audio/wav;base64,";
// Bound 16 kHz mono PCM, including the WAV header and Data URL prefix.
pub(super) const MAX_AUDIO_SAMPLES: usize =
    ((MAX_AUDIO_DATA_BYTES - AUDIO_DATA_PREFIX.len()) / 4 * 3 - 44) / 2;

pub(super) async fn transcribe(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    settings: &RecognitionServiceSettings,
    language: &str,
    samples: &[f32],
    allow_empty: bool,
) -> Result<Transcription, String> {
    let body = request_body(settings, language, samples)?;
    let base = providers::effective_base_url(profile)?;
    let mut request = http
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .bearer_auth(api_key)
        .timeout(Duration::from_millis(profile.timeout_ms))
        .json(&body);
    for custom in &profile.headers {
        let name = reqwest::header::HeaderName::from_bytes(custom.name.trim().as_bytes())
            .map_err(|error| format!("Invalid custom HTTP header: {error}"))?;
        let value = reqwest::header::HeaderValue::from_str(&custom.value)
            .map_err(|error| format!("Invalid custom HTTP header: {error}"))?;
        request = request.header(name, value);
    }
    let response = request.send().await.map_err(network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(status_error(
            status,
            &response.text().await.unwrap_or_default(),
        ));
    }
    let value = response
        .json()
        .await
        .map_err(|error| format!("MiMo transcription returned invalid JSON: {error}"))?;
    parse_transcription(value, language, allow_empty)
}

fn request_body(
    settings: &RecognitionServiceSettings,
    language: &str,
    samples: &[f32],
) -> Result<Value, String> {
    if samples.is_empty() {
        return Err("Cannot transcribe empty audio".into());
    }
    let model = settings.model.trim();
    if model.is_empty() {
        return Err("A transcription model is required".into());
    }
    if !providers::MIMO_ASR_LANGUAGES.contains(&language) {
        return Err(format!(
            "Unsupported MiMo recognition language: {language}. Use auto, zh or en."
        ));
    }
    if samples.len() > MAX_AUDIO_SAMPLES {
        return Err("MiMo transcription audio exceeds the 10 MB Base64 limit".into());
    }
    let data = format!(
        "{AUDIO_DATA_PREFIX}{}",
        STANDARD.encode(encode_wav(samples))
    );
    if data.len() > MAX_AUDIO_DATA_BYTES {
        return Err("MiMo transcription audio exceeds the 10 MB Base64 limit".into());
    }
    Ok(json!({
        "model": model,
        "messages": [{"role": "user", "content": [{
            "type": "input_audio", "input_audio": {"data": data}
        }]}],
        "asr_options": {"language": language},
        "stream": false
    }))
}

fn parse_transcription(
    value: Value,
    language: &str,
    allow_empty: bool,
) -> Result<Transcription, String> {
    if value.get("error").is_some_and(|error| !error.is_null()) {
        return Err(status_error(
            reqwest::StatusCode::BAD_GATEWAY,
            &value.to_string(),
        ));
    }
    if let Some(reason) = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
    {
        if reason != "stop" {
            return Err(format!(
                "MiMo transcription was incomplete (finish_reason={reason})"
            ));
        }
    }
    let text = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or("MiMo transcription response did not contain text")?
        .trim();
    if text.is_empty() && !allow_empty {
        return Err("MiMo transcription response did not contain text".into());
    }
    Ok(Transcription {
        text: text.to_owned(),
        language: (language != "auto").then(|| language.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiProfile, RecognitionServiceSettings};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde_json::{json, Value};

    fn settings() -> RecognitionServiceSettings {
        RecognitionServiceSettings {
            model: "mimo-v2.5-asr".into(),
            context: String::new(),
        }
    }

    #[tokio::test]
    async fn mimo_uploads_wav_to_chat_completions_with_bearer_auth() {
        use axum::{http::HeaderMap, routing::post, Json, Router};
        let app = Router::new().route("/v1/chat/completions", post(|headers: HeaderMap, Json(body): Json<Value>| async move {
            assert_eq!(headers["authorization"], "Bearer test-key");
            assert_eq!(headers["x-test-header"], "test-value");
            assert_eq!(body["model"], "mimo-v2.5-asr");
            assert_eq!(body["stream"], false);
            assert_eq!(body["asr_options"]["language"], "zh");
            assert_eq!(body["messages"].as_array().unwrap().len(), 1);
            assert_eq!(body["messages"][0]["role"], "user");
            let content = &body["messages"][0]["content"][0];
            assert_eq!(content["type"], "input_audio");
            let data = content["input_audio"]["data"].as_str().unwrap();
            let wav = STANDARD.decode(data.strip_prefix("data:audio/wav;base64,").unwrap()).unwrap();
            assert_eq!(&wav[..4], b"RIFF");
            assert_eq!(wav.len(), 44 + 16_000 * 2);
            assert!(body.get("prompt").is_none());
            Json(json!({"choices": [{"message": {"content": "  hello  "}, "finish_reason": "stop"}]}))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let profile = ApiProfile {
            base_url: Some(format!("http://{}/v1", listener.local_addr().unwrap())),
            provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            headers: vec![crate::config::HttpHeaderConfig {
                name: "X-Test-Header".into(),
                value: "test-value".into(),
            }],
            ..ApiProfile::default()
        };
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = transcribe(
            &http,
            &profile,
            "test-key",
            &settings(),
            "zh",
            &vec![0.1; 16_000],
            false,
        )
        .await
        .unwrap();
        assert_eq!(result.text, "hello");
        assert_eq!(result.language.as_deref(), Some("zh"));
        server.abort();
    }

    #[test]
    fn mimo_checks_encoded_size_language_and_empty_audio_before_upload() {
        let settings = settings();
        assert!(request_body(&settings, "auto", &[]).is_err());
        assert!(request_body(&settings, "ja", &[0.1]).is_err());
        assert!(request_body(&RecognitionServiceSettings::default(), "auto", &[0.1]).is_err());
        let max_samples = (10_000_000 / 4 * 3 - 44) / 2;
        assert!(request_body(&settings, "auto", &vec![0.0; max_samples + 1]).is_err());
        let body = request_body(&settings, "auto", &vec![0.0; MAX_AUDIO_SAMPLES]).unwrap();
        assert!(
            body["messages"][0]["content"][0]["input_audio"]["data"]
                .as_str()
                .unwrap()
                .len()
                <= 10_000_000
        );
    }

    #[test]
    fn mimo_requires_a_complete_transcript_and_does_not_invent_detected_language() {
        let value = json!({"choices": [{"message": {"content": "hello", "reasoning_content": "private"}, "finish_reason": "stop"}]});
        let result = parse_transcription(value.clone(), "auto", false).unwrap();
        assert_eq!(result.text, "hello");
        assert_eq!(result.language, None);
        let mut truncated = value;
        truncated["choices"][0]["finish_reason"] = json!("length");
        assert!(parse_transcription(truncated, "auto", false).is_err());
        let silent = json!({"choices": [{"message": {"content": ""}, "finish_reason": "stop"}]});
        assert!(parse_transcription(silent.clone(), "auto", false).is_err());
        assert!(parse_transcription(silent, "auto", true)
            .unwrap()
            .text
            .is_empty());
        assert!(
            parse_transcription(json!({"error": {"message": "rate limit"}}), "auto", true).is_err()
        );
        assert!(parse_transcription(
            json!({"choices": [{"message": {"reasoning_content": "private"}}]}),
            "auto",
            true
        )
        .is_err());
    }
}
