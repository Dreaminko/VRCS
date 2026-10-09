use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use super::transcription::Transcription;
use crate::config::{ApiAuthMode, ApiProfile, RecognitionServiceSettings};
use crate::providers;

const SAMPLE_RATE: u32 = 16_000;
const CHANNELS: u16 = 1;
const BITS_PER_SAMPLE: u16 = 16;
const MAX_ERROR_BODY_CHARS: usize = 8 * 1024;

pub(crate) fn http_client(profile: &ApiProfile) -> reqwest::Client {
    if profile.provider == providers::QWEN_LOCAL_PROVIDER {
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("static local HTTP client options are valid")
    } else {
        reqwest::Client::new()
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
}

#[cfg(test)]
pub(crate) async fn transcribe(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    settings: &RecognitionServiceSettings,
    language: &str,
    samples: &[f32],
) -> Result<String, String> {
    transcribe_result(http, profile, api_key, settings, language, samples)
        .await
        .map(|result| result.text)
}

pub(crate) async fn transcribe_result(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    settings: &RecognitionServiceSettings,
    language: &str,
    samples: &[f32],
) -> Result<Transcription, String> {
    let response =
        request_transcription(http, profile, api_key, settings, language, samples).await?;
    let text = response.text;
    let detected = response
        .language
        .as_deref()
        .and_then(qwen_language_code)
        .or_else(|| {
            if profile.provider != providers::QWEN_LOCAL_PROVIDER {
                return None;
            }
            text.trim()
                .strip_prefix("language ")
                .and_then(|marked| marked.split_once("<asr_text>"))
                .and_then(|(name, _)| qwen_language_code(name.trim()))
        });
    let text = if profile.provider == providers::QWEN_LOCAL_PROVIDER {
        normalize_local_qwen_text(&text)?
    } else {
        text.trim()
    };
    if text.is_empty() && profile.provider != providers::QWEN_LOCAL_PROVIDER {
        return Err("Transcription response did not contain text".into());
    }
    Ok(Transcription {
        text: text.to_owned(),
        language: if language == "auto" {
            detected.map(str::to_owned)
        } else {
            Some(language.to_owned())
        },
    })
}

pub(crate) async fn transcribe_managed_qwen(
    connection: &super::qwen_runtime::QwenConnection,
    language: &str,
    samples: &[f32],
) -> Result<Transcription, String> {
    let profile = ApiProfile {
        provider: providers::QWEN_LOCAL_PROVIDER.into(),
        base_url: Some(connection.base_url.clone()),
        auth_mode: ApiAuthMode::Bearer,
        timeout_ms: 120_000,
        ..ApiProfile::default()
    };
    let settings = RecognitionServiceSettings {
        model: connection.model.clone(),
        context: String::new(),
    };
    transcribe_result(
        &http_client(&profile),
        &profile,
        &connection.token,
        &settings,
        language,
        samples,
    )
    .await
}

fn qwen_language_code(language: &str) -> Option<&'static str> {
    match language.to_ascii_lowercase().as_str() {
        "chinese" | "zh" => Some("zh"),
        "english" | "en" => Some("en"),
        "japanese" | "ja" => Some("ja"),
        "korean" | "ko" => Some("ko"),
        "spanish" | "es" => Some("es"),
        "french" | "fr" => Some("fr"),
        "german" | "de" => Some("de"),
        "arabic" | "ar" => Some("ar"),
        "cantonese" | "yue" => Some("yue"),
        "russian" | "ru" => Some("ru"),
        "portuguese" | "pt" => Some("pt"),
        "italian" | "it" => Some("it"),
        "indonesian" | "id" => Some("id"),
        "thai" | "th" => Some("th"),
        "vietnamese" | "vi" => Some("vi"),
        "turkish" | "tr" => Some("tr"),
        "hindi" | "hi" => Some("hi"),
        "malay" | "ms" => Some("ms"),
        "dutch" | "nl" => Some("nl"),
        "swedish" | "sv" => Some("sv"),
        "danish" | "da" => Some("da"),
        "finnish" | "fi" => Some("fi"),
        "polish" | "pl" => Some("pl"),
        "czech" | "cs" => Some("cs"),
        "filipino" | "tagalog" | "tl" => Some("tl"),
        "persian" | "fa" => Some("fa"),
        "greek" | "el" => Some("el"),
        "hungarian" | "hu" => Some("hu"),
        "macedonian" | "mk" => Some("mk"),
        "romanian" | "ro" => Some("ro"),
        _ => None,
    }
}

fn normalize_local_qwen_text(text: &str) -> Result<&str, String> {
    let text = text.trim();
    if let Some(text) = text.strip_prefix("<asr_text>") {
        return Ok(text.trim());
    }
    if let Some(marked) = text.strip_prefix("language ") {
        if let Some((language, transcript)) = marked.split_once("<asr_text>") {
            if language.trim().is_empty() {
                return Err("Local Qwen ASR returned an empty language marker".into());
            }
            return Ok(transcript.trim());
        }
    }
    if text.contains("<asr_text") {
        return Err("Local Qwen ASR returned an unexpected text marker".into());
    }
    Ok(text)
}

pub(crate) async fn test_connection(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    settings: &RecognitionServiceSettings,
    language: &str,
    samples: &[f32],
) -> Result<(), String> {
    request_transcription(http, profile, api_key, settings, language, samples)
        .await
        .map(|_| ())
}

async fn request_transcription(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    settings: &RecognitionServiceSettings,
    language: &str,
    samples: &[f32],
) -> Result<TranscriptionResponse, String> {
    if samples.is_empty() {
        return Err("Cannot transcribe empty audio".into());
    }
    let model = settings.model.trim();
    if model.is_empty() {
        return Err("A transcription model is required".into());
    }

    let form = transcription_form(
        model,
        settings.context.trim(),
        language,
        encode_wav(samples),
    );
    let mut request = http
        .post(transcriptions_url(profile)?)
        .timeout(Duration::from_millis(profile.timeout_ms))
        .multipart(form);
    if profile.requires_api_key() {
        request = request.bearer_auth(api_key);
    }
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
        let body = response.text().await.unwrap_or_default();
        return Err(status_error(status, &body));
    }
    let response: TranscriptionResponse = response
        .json()
        .await
        .map_err(|error| format!("Transcription returned invalid JSON: {error}"))?;
    Ok(response)
}

fn transcription_form(model: &str, context: &str, language: &str, wav: Vec<u8>) -> Form {
    let mut form = Form::new()
        .part(
            "file",
            Part::bytes(wav)
                .file_name("utterance.wav")
                .mime_str("audio/wav")
                .expect("static WAV MIME type"),
        )
        .text("model", model.to_owned())
        .text("response_format", "json");
    if language != "auto" {
        form = form.text("language", language.to_owned());
    }
    if !context.is_empty() {
        form = form.text("prompt", context.to_owned());
    }
    form
}

fn transcriptions_url(profile: &ApiProfile) -> Result<String, String> {
    let base = providers::effective_base_url(profile)?;
    let mut url = reqwest::Url::parse(&base)
        .map_err(|error| format!("The transcription Base URL is invalid: {error}"))?;
    let path = format!("{}/audio/transcriptions", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url.into())
}

pub(crate) fn encode_wav(samples: &[f32]) -> Vec<u8> {
    let data_size = samples.len().saturating_mul(2).min(u32::MAX as usize) as u32;
    let riff_size = 36u32.saturating_add(data_size);
    let byte_rate = SAMPLE_RATE * u32::from(CHANNELS) * u32::from(BITS_PER_SAMPLE) / 8;
    let block_align = CHANNELS * BITS_PER_SAMPLE / 8;
    let mut wav = Vec::with_capacity(44 + data_size as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&CHANNELS.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples.iter().take(data_size as usize / 2) {
        let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        wav.extend_from_slice(&pcm.to_le_bytes());
    }
    wav
}

fn network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "Transcription request timed out".into()
    } else {
        format!("Transcription request failed: {error}")
    }
}

fn status_error(status: reqwest::StatusCode, body: &str) -> String {
    let detail = body
        .trim()
        .chars()
        .take(MAX_ERROR_BODY_CHARS)
        .collect::<String>();
    let summary = match status.as_u16() {
        401 | 403 => "Transcription authentication failed",
        404 => "Transcription endpoint or model was not found",
        413 => "Transcription audio payload is too large",
        429 => "Transcription rate limit was exceeded",
        500..=599 => "Transcription service is unavailable",
        _ => "Transcription request was rejected",
    };
    if detail.is_empty() {
        format!("{summary} (HTTP {status})")
    } else {
        format!("{summary} (HTTP {status}): {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiAuthMode, ApiProfile, RecognitionServiceSettings};
    use crate::providers::{GROQ_PROVIDER, OPENAI_COMPATIBLE_PROVIDER};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn mock_server(
        response_body: &'static str,
    ) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            request
        });
        (format!("http://{address}/v1"), task)
    }

    async fn read_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut expected = None;
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "HTTP request closed before its body was complete");
            request.extend_from_slice(&buffer[..read]);
            if expected.is_none() {
                if let Some(header_end) = request.windows(4).position(|value| value == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::trim)
                                .map(str::to_owned)
                        })
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap();
                    expected = Some(header_end + 4 + content_length);
                }
            }
            if expected.is_some_and(|length| request.len() >= length) {
                return request;
            }
        }
    }

    fn mock_profile(base_url: String) -> ApiProfile {
        ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(base_url),
            auth_mode: ApiAuthMode::None,
            ..ApiProfile::default()
        }
    }

    #[tokio::test]
    async fn managed_qwen_reuses_multipart_transcription_with_runtime_credentials() {
        let (base_url, server) = mock_server(r#"{"text":"<asr_text>你好"}"#).await;
        let connection = super::super::qwen_runtime::QwenConnection {
            base_url,
            token: "runtime-token".into(),
            model: "vrcs-qwen".into(),
            generation: 1,
            session: 1,
        };
        let text = transcribe_managed_qwen(&connection, "auto", &[0.1, -0.1])
            .await
            .unwrap();
        assert_eq!(text.text, "你好");
        let request = String::from_utf8_lossy(&server.await.unwrap()).to_lowercase();
        assert!(request.contains("authorization: bearer runtime-token"));
        assert!(request.contains("name=\"model\""));
        assert!(request.contains("vrcs-qwen"));
    }

    #[test]
    fn wav_encoder_writes_pcm16_mono_header() {
        let wav = encode_wav(&[-1.0, 0.0, 1.0]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            16_000
        );
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6);
        assert_eq!(wav.len(), 50);
    }

    #[test]
    fn transcription_url_appends_audio_path() {
        let profile = ApiProfile {
            provider: GROQ_PROVIDER.into(),
            ..ApiProfile::default()
        };
        assert_eq!(
            transcriptions_url(&profile).unwrap(),
            "https://api.groq.com/openai/v1/audio/transcriptions"
        );
    }

    #[tokio::test]
    async fn multipart_request_omits_auto_language_and_includes_context() {
        let (base_url, request) = mock_server(r#"{"text":"hello"}"#).await;
        let settings = RecognitionServiceSettings {
            model: "whisper-test".into(),
            context: "VRChat names".into(),
        };
        let mut profile = mock_profile(base_url);
        profile.auth_mode = ApiAuthMode::Bearer;
        let text = transcribe(
            &reqwest::Client::new(),
            &profile,
            "test-key",
            &settings,
            "auto",
            &[0.1; 320],
        )
        .await
        .unwrap();
        assert_eq!(text, "hello");
        let request_bytes = request.await.unwrap();
        let request = String::from_utf8_lossy(&request_bytes);
        assert!(request.contains("authorization: Bearer test-key"));
        assert!(request.contains("name=\"file\"; filename=\"utterance.wav\""));
        assert!(request.contains("name=\"model\""));
        assert!(request.contains("whisper-test"));
        assert!(request.contains("name=\"prompt\""));
        assert!(request.contains("VRChat names"));
        assert!(request.contains("name=\"response_format\""));
        assert!(!request.contains("name=\"language\""));
    }

    #[tokio::test]
    async fn runtime_rejects_empty_text_but_connection_test_accepts_the_response() {
        let settings = RecognitionServiceSettings {
            model: "whisper-test".into(),
            context: String::new(),
        };
        let (base_url, request) = mock_server(r#"{"text":""}"#).await;
        let error = transcribe(
            &reqwest::Client::new(),
            &mock_profile(base_url),
            "",
            &settings,
            "en",
            &[0.1; 320],
        )
        .await
        .unwrap_err();
        assert!(error.contains("did not contain text"));
        let request_bytes = request.await.unwrap();
        let request = String::from_utf8_lossy(&request_bytes);
        assert!(request.contains("name=\"language\""));
        assert!(request.contains("\r\n\r\nen\r\n"));

        let (base_url, _) = mock_server(r#"{"text":""}"#).await;
        test_connection(
            &reqwest::Client::new(),
            &mock_profile(base_url),
            "",
            &settings,
            "auto",
            &[0.1; 320],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn local_qwen_silence_returns_an_empty_final_without_changing_cloud_behavior() {
        let settings = RecognitionServiceSettings {
            model: "Qwen/Qwen3-ASR-0.6B".into(),
            context: String::new(),
        };
        let (base_url, request) = mock_server(r#"{"text":""}"#).await;
        let profile = ApiProfile {
            provider: crate::providers::QWEN_LOCAL_PROVIDER.into(),
            base_url: Some(base_url),
            auth_mode: ApiAuthMode::None,
            is_local: true,
            ..ApiProfile::default()
        };
        assert_eq!(
            transcribe(
                &reqwest::Client::new(),
                &profile,
                "",
                &settings,
                "auto",
                &[0.1; 320]
            )
            .await
            .unwrap(),
            ""
        );
        let request = String::from_utf8_lossy(&request.await.unwrap()).to_string();
        assert!(request.starts_with("POST /v1/audio/transcriptions HTTP/1.1"));
        assert!(request.contains("name=\"model\""));
        assert!(!request.contains("authorization:"));
    }

    #[tokio::test]
    async fn local_qwen_strips_the_asr_language_prefix() {
        let (base_url, _) =
            mock_server(r#"{"text":"language English<asr_text>Hello VRChat"}"#).await;
        let profile = ApiProfile {
            provider: crate::providers::QWEN_LOCAL_PROVIDER.into(),
            base_url: Some(base_url),
            auth_mode: ApiAuthMode::None,
            is_local: true,
            ..ApiProfile::default()
        };
        let settings = RecognitionServiceSettings {
            model: "Qwen/Qwen3-ASR-0.6B".into(),
            context: String::new(),
        };
        assert_eq!(
            transcribe(
                &http_client(&profile),
                &profile,
                "",
                &settings,
                "auto",
                &[0.1; 320]
            )
            .await
            .unwrap(),
            "Hello VRChat"
        );
    }

    #[test]
    fn malformed_qwen_asr_prefix_is_not_shown_as_a_subtitle() {
        assert!(normalize_local_qwen_text("language English<asr_text>")
            .unwrap()
            .is_empty());
        assert!(normalize_local_qwen_text("language English<asr_text>Hello").is_ok());
        assert!(normalize_local_qwen_text("language English<asr_text").is_err());
        assert_eq!(
            normalize_local_qwen_text("language English is useful").unwrap(),
            "language English is useful"
        );
        assert_eq!(
            normalize_local_qwen_text("normal transcript").unwrap(),
            "normal transcript"
        );
    }

    #[tokio::test]
    async fn local_qwen_does_not_follow_audio_redirects() {
        let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let destination_url = format!(
            "http://{}/v1/audio/transcriptions",
            destination.local_addr().unwrap()
        );
        let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", source.local_addr().unwrap());
        let source_task = tokio::spawn(async move {
            let (mut stream, _) = source.accept().await.unwrap();
            read_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: {destination_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let profile = ApiProfile {
            provider: crate::providers::QWEN_LOCAL_PROVIDER.into(),
            base_url: Some(base_url),
            auth_mode: ApiAuthMode::None,
            is_local: true,
            ..ApiProfile::default()
        };
        let settings = RecognitionServiceSettings {
            model: "Qwen/Qwen3-ASR-0.6B".into(),
            context: String::new(),
        };
        let error = transcribe(
            &http_client(&profile),
            &profile,
            "",
            &settings,
            "auto",
            &[0.1; 320],
        )
        .await
        .unwrap_err();
        assert!(error.contains("HTTP 307"), "{error}");
        assert!(!error.contains("Cloud"), "{error}");
        source_task.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), destination.accept())
                .await
                .is_err()
        );
    }

    #[test]
    fn status_errors_identify_common_failures() {
        assert!(status_error(reqwest::StatusCode::UNAUTHORIZED, "bad key")
            .contains("authentication failed"));
        assert!(status_error(reqwest::StatusCode::PAYLOAD_TOO_LARGE, "")
            .contains("payload is too large"));
        assert!(status_error(reqwest::StatusCode::TOO_MANY_REQUESTS, "").contains("rate limit"));
    }
}
