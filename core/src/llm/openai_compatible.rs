use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header;
use serde_json::{json, Value};

use crate::config::ApiProfile;

use super::http::{
    invalid_response, list_models as parse_models, network_error, response_status_error,
};
use super::{LlmError, LlmProgress, LlmRequest};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProtocolBehavior {
    Standard,
    DeepSeek,
}

pub(super) async fn generate(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    generate_chat_completion(
        http,
        chat_completions_url(profile)?,
        api_key,
        request,
        "OpenAI-compatible",
        protocol_behavior(profile),
        Some(profile),
        on_progress,
    )
    .await
}

pub(super) async fn generate_standard(
    http: &reqwest::Client,
    endpoint: String,
    api_key: &str,
    request: LlmRequest<'_>,
    provider_name: &str,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    generate_chat_completion(
        http,
        endpoint,
        api_key,
        request,
        provider_name,
        ProtocolBehavior::Standard,
        None,
        on_progress,
    )
    .await
}

pub(super) async fn list_models(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
) -> Result<Vec<String>, LlmError> {
    let request = apply_profile_request(http.get(models_url(profile)?), profile, api_key)?;
    parse_models(request).await
}

pub(super) async fn test_streaming(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    on_progress: &LlmProgress,
) -> Result<String, LlmError> {
    let body = chat_completion_body(&request, protocol_behavior(profile), true);
    let response = apply_profile_request(
        http.post(chat_completions_url(profile)?).json(&body),
        profile,
        api_key,
    )?
    .send()
    .await
    .map_err(network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(response_status_error(response, status).await);
    }
    if !is_event_stream(&response) {
        return Err(LlmError {
            code: "llm.sse_incompatible",
            detail: "The service did not return a text/event-stream response".into(),
            retryable: false,
        });
    }
    stream_chat_completion(response, on_progress, "OpenAI-compatible", request.model).await
}

#[allow(clippy::too_many_arguments)]
async fn generate_chat_completion(
    http: &reqwest::Client,
    endpoint: String,
    api_key: &str,
    request: LlmRequest<'_>,
    provider_name: &str,
    behavior: ProtocolBehavior,
    profile: Option<&ApiProfile>,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    let body = chat_completion_body(&request, behavior, on_progress.is_some());
    let builder = http.post(endpoint).json(&body);
    let builder = if let Some(profile) = profile {
        apply_profile_request(builder, profile, api_key)?
    } else {
        builder.bearer_auth(api_key)
    };
    let response = builder.send().await.map_err(network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(response_status_error(response, status).await);
    }
    if let Some(progress) = on_progress.filter(|_| is_event_stream(&response)) {
        return stream_chat_completion(response, progress, provider_name, request.model).await;
    }
    let value: Value = response.json().await.map_err(invalid_response)?;
    trace_usage(&value, provider_name, request.model);
    let text = extract_text(&value).ok_or_else(|| LlmError {
        code: "llm.invalid_response",
        detail: format!("{provider_name} response did not contain text"),
        retryable: false,
    })?;
    if let Some(progress) = on_progress {
        progress(&text);
    }
    Ok(text)
}

fn chat_completion_body(
    request: &LlmRequest<'_>,
    behavior: ProtocolBehavior,
    stream: bool,
) -> Value {
    let mut body = json!({
        "model": request.model,
        "messages": [
            { "role": "system", "content": request.instructions },
            { "role": "user", "content": request.input }
        ],
        "max_tokens": request.max_output_tokens,
        "stream": stream
    });
    if behavior == ProtocolBehavior::DeepSeek {
        body["thinking"] = json!({
            "type": if request.thinking_enabled { "enabled" } else { "disabled" }
        });
        if !request.thinking_enabled {
            body["temperature"] = json!(0);
        }
        if stream {
            body["stream_options"] = json!({ "include_usage": true });
        }
    }
    body
}

fn protocol_behavior(profile: &ApiProfile) -> ProtocolBehavior {
    if profile.preset_id.as_deref() == Some("deepseek") {
        ProtocolBehavior::DeepSeek
    } else {
        ProtocolBehavior::Standard
    }
}

fn apply_profile_request(
    mut builder: reqwest::RequestBuilder,
    profile: &ApiProfile,
    api_key: &str,
) -> Result<reqwest::RequestBuilder, LlmError> {
    builder = builder.timeout(Duration::from_millis(profile.timeout_ms));
    if profile.requires_api_key() {
        builder = builder.bearer_auth(api_key);
    }
    for custom in &profile.headers {
        let name = reqwest::header::HeaderName::from_bytes(custom.name.trim().as_bytes()).map_err(
            |error| LlmError {
                code: "llm.invalid_profile",
                detail: error.to_string(),
                retryable: false,
            },
        )?;
        let value =
            reqwest::header::HeaderValue::from_str(&custom.value).map_err(|error| LlmError {
                code: "llm.invalid_profile",
                detail: error.to_string(),
                retryable: false,
            })?;
        builder = builder.header(name, value);
    }
    Ok(builder)
}

async fn stream_chat_completion(
    response: reqwest::Response,
    on_progress: &LlmProgress,
    provider_name: &str,
    model: &str,
) -> Result<String, LlmError> {
    let mut chunks = response.bytes_stream();
    let mut buffer = Vec::<u8>::new();
    let mut output = String::new();
    while let Some(chunk) = chunks.next().await {
        buffer.extend_from_slice(&chunk.map_err(network_error)?);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line = buffer.drain(..=newline).collect::<Vec<_>>();
            if process_sse_line(&line, &mut output, on_progress, provider_name, model)? {
                return completed_stream(output, provider_name);
            }
        }
    }
    if !buffer.is_empty() {
        let _ = process_sse_line(&buffer, &mut output, on_progress, provider_name, model)?;
    }
    completed_stream(output, provider_name)
}

fn process_sse_line(
    line: &[u8],
    output: &mut String,
    on_progress: &LlmProgress,
    provider_name: &str,
    model: &str,
) -> Result<bool, LlmError> {
    let line = std::str::from_utf8(line).map_err(|error| LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
    })?;
    let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else {
        return Ok(false);
    };
    if data == "[DONE]" {
        return Ok(true);
    }
    let value: Value = serde_json::from_str(data).map_err(|error| LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
    })?;
    trace_usage(&value, provider_name, model);
    if let Some(delta) = extract_delta(&value) {
        output.push_str(&delta);
        on_progress(output);
    }
    Ok(false)
}

fn completed_stream(output: String, provider_name: &str) -> Result<String, LlmError> {
    let output = output.trim().to_owned();
    if output.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: format!("{provider_name} stream did not contain text"),
            retryable: false,
        });
    }
    Ok(output)
}

fn is_event_stream(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"))
}

fn chat_completions_url(profile: &ApiProfile) -> Result<String, LlmError> {
    let base_url = profile
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(missing_base_url)?
        .trim_end_matches('/');
    if base_url.ends_with("/chat/completions") {
        Ok(base_url.into())
    } else {
        Ok(format!("{base_url}/chat/completions"))
    }
}

fn models_url(profile: &ApiProfile) -> Result<String, LlmError> {
    let base_url = profile
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(missing_base_url)?
        .trim_end_matches('/');
    let base_url = base_url
        .strip_suffix("/chat/completions")
        .unwrap_or(base_url)
        .trim_end_matches('/');
    if base_url.ends_with("/models") {
        Ok(base_url.into())
    } else {
        Ok(format!("{base_url}/models"))
    }
}

fn missing_base_url() -> LlmError {
    LlmError {
        code: "llm.invalid_profile",
        detail: "OpenAI-compatible Base URL is missing".into(),
        retryable: false,
    }
}

fn extract_text(value: &Value) -> Option<String> {
    let content = value.pointer("/choices/0/message/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.trim().to_owned()).filter(|text| !text.is_empty());
    }
    let text = content
        .as_array()?
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>()
        .trim()
        .to_owned();
    Some(text).filter(|text| !text.is_empty())
}

fn extract_delta(value: &Value) -> Option<String> {
    let content = value.pointer("/choices/0/delta/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_owned()).filter(|text| !text.is_empty());
    }
    let text = content
        .as_array()?
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    Some(text).filter(|text| !text.is_empty())
}

fn trace_usage(value: &Value, provider_name: &str, model: &str) {
    let Some(usage) = value.get("usage") else {
        return;
    };
    tracing::debug!(
        provider = provider_name,
        model,
        prompt_tokens = usage
            .get("prompt_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        completion_tokens = usage
            .get("completion_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        reasoning_tokens = usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        prompt_cache_hit_tokens = usage
            .get("prompt_cache_hit_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        prompt_cache_miss_tokens = usage
            .get("prompt_cache_miss_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        "LLM token usage"
    );
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::config::{ApiAuthMode, HttpHeaderConfig};
    use crate::providers::OPENAI_COMPATIBLE_PROVIDER;

    #[test]
    fn extracts_chat_completions_text() {
        let value = json!({
            "choices": [{"message": {"role": "assistant", "content": " hello "}}]
        });
        assert_eq!(extract_text(&value).as_deref(), Some("hello"));
    }

    #[test]
    fn deepseek_body_disables_thinking_and_bounds_output() {
        let request = LlmRequest {
            model: "deepseek-v4-flash",
            instructions: "Translate",
            input: "hello",
            max_output_tokens: 128,
            thinking_enabled: false,
        };
        let body = chat_completion_body(&request, ProtocolBehavior::DeepSeek, true);
        assert_eq!(body.pointer("/thinking/type"), Some(&json!("disabled")));
        assert_eq!(body.get("temperature"), Some(&json!(0)));
        assert_eq!(body.get("max_tokens"), Some(&json!(128)));
        assert_eq!(body.get("stream"), Some(&json!(true)));

        let generic = chat_completion_body(&request, ProtocolBehavior::Standard, false);
        assert!(generic.get("thinking").is_none());
        assert!(generic.get("temperature").is_none());

        let mut profile = ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some("https://api.deepseek.com/v1".into()),
            ..ApiProfile::default()
        };
        assert!(matches!(
            protocol_behavior(&profile),
            ProtocolBehavior::Standard
        ));
        profile.preset_id = Some("deepseek".into());
        assert!(matches!(
            protocol_behavior(&profile),
            ProtocolBehavior::DeepSeek
        ));
    }

    #[test]
    fn accumulates_streamed_content() {
        let captured = Arc::new(Mutex::new(String::new()));
        let progress_target = Arc::clone(&captured);
        let progress = move |text: &str| {
            *progress_target.lock().unwrap() = text.to_owned();
        };
        let mut output = String::new();
        assert!(!process_sse_line(
            br#"data: {"choices":[{"delta":{"content":"\u4f60"}}]}"#,
            &mut output,
            &progress,
            "DeepSeek",
            "deepseek-v4-flash",
        )
        .unwrap());
        assert!(!process_sse_line(
            br#"data: {"choices":[{"delta":{"content":"\u597d"}}]}"#,
            &mut output,
            &progress,
            "DeepSeek",
            "deepseek-v4-flash",
        )
        .unwrap());
        assert_eq!(output, "你好");
        assert_eq!(*captured.lock().unwrap(), "你好");
        assert!(process_sse_line(
            b"data: [DONE]\n",
            &mut output,
            &progress,
            "DeepSeek",
            "deepseek-v4-flash",
        )
        .unwrap());
    }

    #[test]
    fn builds_chat_and_model_endpoints() {
        let mut profile = ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some("https://api.deepseek.com/v1/".into()),
            ..ApiProfile::default()
        };
        assert_eq!(
            chat_completions_url(&profile).unwrap(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            models_url(&profile).unwrap(),
            "https://api.deepseek.com/v1/models"
        );
        profile.base_url = Some("https://example.com/v1/chat/completions".into());
        assert_eq!(
            models_url(&profile).unwrap(),
            "https://example.com/v1/models"
        );
    }

    #[tokio::test]
    async fn no_auth_profile_sends_custom_headers_without_authorization() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let read = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 23\r\nConnection: close\r\n\r\n{\"data\":[{\"id\":\"one\"}]}")
                .await
                .unwrap();
            String::from_utf8_lossy(&request[..read]).to_string()
        });
        let profile = ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(format!("http://{address}/v1")),
            auth_mode: ApiAuthMode::None,
            is_local: true,
            headers: vec![HttpHeaderConfig {
                name: "X-Test-Client".into(),
                value: "VRCS".into(),
            }],
            ..ApiProfile::default()
        };
        let models = list_models(&reqwest::Client::new(), &profile, "")
            .await
            .unwrap();
        let request = server.await.unwrap().to_ascii_lowercase();
        assert_eq!(models, ["one"]);
        assert!(request.contains("x-test-client: vrcs"));
        assert!(!request.contains("authorization:"));
    }
}
