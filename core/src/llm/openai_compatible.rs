use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header;
use serde_json::{json, Value};

use crate::config::ApiProfile;
use crate::providers::{self, OpenAiProtocolBehavior, OPENROUTER_PROVIDER};

use super::http::{
    invalid_response, list_models as parse_models, network_error, response_status_error,
};
use super::reasoning::{self, InlineReasoningFilter};
use super::{LlmError, LlmProgress, LlmRequest};

pub(super) async fn generate(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    provider_name: &str,
    behavior: OpenAiProtocolBehavior,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    generate_chat_completion(
        http,
        chat_completions_url(profile)?,
        api_key,
        request,
        provider_name,
        behavior,
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
    behavior: OpenAiProtocolBehavior,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    generate_chat_completion(
        http,
        endpoint,
        api_key,
        request,
        provider_name,
        behavior,
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
    if profile.provider != OPENROUTER_PROVIDER {
        return parse_models(request).await;
    }
    let response = request.send().await.map_err(network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(response_status_error(response, status).await);
    }
    let value: Value = response.json().await.map_err(invalid_response)?;
    let models = extract_openrouter_models(&value);
    if models.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: "OpenRouter did not return any models with text output".into(),
            retryable: false,
        });
    }
    Ok(models)
}

pub(super) async fn test_streaming(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    provider_name: &str,
    behavior: OpenAiProtocolBehavior,
    on_progress: &LlmProgress,
) -> Result<String, LlmError> {
    let body = chat_completion_body(&request, behavior, true);
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
    stream_chat_completion(
        response,
        on_progress,
        provider_name,
        behavior,
        request.model,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn generate_chat_completion(
    http: &reqwest::Client,
    endpoint: String,
    api_key: &str,
    request: LlmRequest<'_>,
    provider_name: &str,
    behavior: OpenAiProtocolBehavior,
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
        return stream_chat_completion(response, progress, provider_name, behavior, request.model)
            .await;
    }
    let value: Value = response.json().await.map_err(invalid_response)?;
    trace_usage(&value, provider_name, request.model);
    let text = extract_text(&value).ok_or_else(|| LlmError {
        code: "llm.invalid_response",
        detail: format!("{provider_name} response did not contain text"),
        retryable: false,
    })?;
    let text = reasoning::sanitize_response_text(behavior, request.model, &text);
    if text.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: format!("{provider_name} response did not contain final answer text"),
            retryable: false,
        });
    }
    if let Some(progress) = on_progress {
        progress(&text);
    }
    Ok(text)
}

fn chat_completion_body(
    request: &LlmRequest<'_>,
    behavior: OpenAiProtocolBehavior,
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
    reasoning::apply_chat_completion_reasoning(
        &mut body,
        behavior,
        request.model,
        request.thinking_enabled,
        stream,
    );
    body
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
    behavior: OpenAiProtocolBehavior,
    model: &str,
) -> Result<String, LlmError> {
    let mut chunks = response.bytes_stream();
    let mut buffer = Vec::<u8>::new();
    let mut output = String::new();
    let mut filter = reasoning::should_filter_inline_reasoning(behavior, model)
        .then(InlineReasoningFilter::default);
    while let Some(chunk) = chunks.next().await {
        buffer.extend_from_slice(&chunk.map_err(network_error)?);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line = buffer.drain(..=newline).collect::<Vec<_>>();
            if process_sse_line(
                &line,
                &mut output,
                filter.as_mut(),
                on_progress,
                provider_name,
                model,
            )? {
                return completed_filtered_stream(output, filter, on_progress, provider_name);
            }
        }
    }
    if !buffer.is_empty() {
        let _ = process_sse_line(
            &buffer,
            &mut output,
            filter.as_mut(),
            on_progress,
            provider_name,
            model,
        )?;
    }
    completed_filtered_stream(output, filter, on_progress, provider_name)
}

fn process_sse_line(
    line: &[u8],
    output: &mut String,
    filter: Option<&mut InlineReasoningFilter>,
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
        let delta = match filter {
            Some(filter) => filter.push(&delta),
            None => delta,
        };
        if !delta.is_empty() {
            output.push_str(&delta);
            on_progress(output);
        }
    }
    Ok(false)
}

fn completed_filtered_stream(
    mut output: String,
    filter: Option<InlineReasoningFilter>,
    on_progress: &LlmProgress,
    provider_name: &str,
) -> Result<String, LlmError> {
    if let Some(mut filter) = filter {
        let remaining = filter.finish();
        if !remaining.is_empty() {
            output.push_str(&remaining);
            on_progress(&output);
        }
    }
    completed_stream(output, provider_name)
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
    let base_url = providers::effective_base_url(profile).map_err(invalid_profile)?;
    let base_url = base_url.trim_end_matches('/');
    if base_url.ends_with("/chat/completions") {
        Ok(base_url.into())
    } else {
        Ok(format!("{base_url}/chat/completions"))
    }
}

fn models_url(profile: &ApiProfile) -> Result<String, LlmError> {
    let base_url = providers::effective_base_url(profile).map_err(invalid_profile)?;
    let base_url = base_url.trim_end_matches('/');
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

fn invalid_profile(detail: String) -> LlmError {
    LlmError {
        code: "llm.invalid_profile",
        detail,
        retryable: false,
    }
}

fn extract_openrouter_models(value: &Value) -> Vec<String> {
    let mut models = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry
                .pointer("/architecture/output_modalities")
                .and_then(Value::as_array)
                .is_some_and(|modalities| modalities.iter().any(|modality| modality == "text"))
        })
        .filter_map(|entry| entry.get("id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|model| !model.is_empty() && model.len() <= 200 && !model.ends_with(":batch"))
        .map(str::to_owned)
        .take(500)
        .collect::<Vec<_>>();
    models.sort_by_key(|model| model.to_ascii_lowercase());
    models.dedup();
    models
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
    use crate::providers::{GROQ_PROVIDER, OPENAI_COMPATIBLE_PROVIDER};

    #[test]
    fn filters_openrouter_models_to_text_output_and_excludes_batch_variants() {
        let models = extract_openrouter_models(&json!({ "data": [
            { "id": "text-model", "architecture": { "output_modalities": ["text"] } },
            { "id": "mixed-model", "architecture": { "output_modalities": ["text", "image"] } },
            { "id": "image-model", "architecture": { "output_modalities": ["image"] } },
            { "id": "text-model:batch", "architecture": { "output_modalities": ["text"] } }
        ] }));

        assert_eq!(models, ["mixed-model", "text-model"]);
    }

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
        let body = chat_completion_body(&request, OpenAiProtocolBehavior::DeepSeek, true);
        assert_eq!(body.pointer("/thinking/type"), Some(&json!("disabled")));
        assert_eq!(body.get("temperature"), Some(&json!(0)));
        assert_eq!(body.get("max_tokens"), Some(&json!(128)));
        assert_eq!(body.get("stream"), Some(&json!(true)));

        let generic = chat_completion_body(&request, OpenAiProtocolBehavior::Standard, false);
        assert!(generic.get("thinking").is_none());
        assert!(generic.get("temperature").is_none());

        let mut profile = ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some("https://api.deepseek.com/v1".into()),
            ..ApiProfile::default()
        };
        profile.preset_id = Some("deepseek".into());
        let generic = chat_completion_body(&request, OpenAiProtocolBehavior::Standard, true);
        assert!(generic.get("thinking").is_none());
    }

    #[test]
    fn maps_groq_reasoning_models_without_conflicting_parameters() {
        let request = |model, thinking_enabled| LlmRequest {
            model,
            instructions: "Translate",
            input: "hello",
            max_output_tokens: 128,
            thinking_enabled,
        };

        let gpt_oss = chat_completion_body(
            &request("openai/gpt-oss-120b", false),
            OpenAiProtocolBehavior::Groq,
            true,
        );
        assert_eq!(gpt_oss.get("include_reasoning"), Some(&json!(false)));
        assert!(gpt_oss.get("reasoning_format").is_none());
        assert!(gpt_oss.get("reasoning_effort").is_none());

        let qwen_disabled = chat_completion_body(
            &request("qwen/qwen3.6-27b", false),
            OpenAiProtocolBehavior::Groq,
            true,
        );
        assert_eq!(
            qwen_disabled.get("reasoning_format"),
            Some(&json!("hidden"))
        );
        assert_eq!(qwen_disabled.get("reasoning_effort"), Some(&json!("none")));
        assert!(qwen_disabled.get("include_reasoning").is_none());

        let qwen_enabled = chat_completion_body(
            &request("qwen/qwen3.6-27b", true),
            OpenAiProtocolBehavior::Groq,
            true,
        );
        assert_eq!(
            qwen_enabled.get("reasoning_effort"),
            Some(&json!("default"))
        );
    }

    #[test]
    fn only_alibaba_hybrid_models_receive_enable_thinking() {
        let request = |model| LlmRequest {
            model,
            instructions: "Translate",
            input: "hello",
            max_output_tokens: 128,
            thinking_enabled: false,
        };

        let hybrid =
            chat_completion_body(&request("qwen-plus"), OpenAiProtocolBehavior::Alibaba, true);
        assert_eq!(hybrid.get("enable_thinking"), Some(&json!(false)));

        let thinking_only = chat_completion_body(
            &request("deepseek-r1"),
            OpenAiProtocolBehavior::Alibaba,
            true,
        );
        assert!(thinking_only.get("enable_thinking").is_none());

        let custom = chat_completion_body(
            &request("qwen/qwen3.6-27b"),
            OpenAiProtocolBehavior::Standard,
            true,
        );
        assert!(custom.get("enable_thinking").is_none());
        assert!(custom.get("reasoning_format").is_none());
        assert!(custom.get("reasoning_effort").is_none());
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
            None,
            &progress,
            "DeepSeek",
            "deepseek-v4-flash",
        )
        .unwrap());
        assert!(!process_sse_line(
            br#"data: {"choices":[{"delta":{"content":"\u597d"}}]}"#,
            &mut output,
            None,
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
            None,
            &progress,
            "DeepSeek",
            "deepseek-v4-flash",
        )
        .unwrap());
    }

    #[test]
    fn filters_streamed_inline_reasoning_before_progress() {
        let snapshots = Arc::new(Mutex::new(Vec::new()));
        let progress_target = Arc::clone(&snapshots);
        let progress = move |text: &str| {
            progress_target.lock().unwrap().push(text.to_owned());
        };
        let mut filter = InlineReasoningFilter::default();
        let mut output = String::new();

        for content in [
            "<thi",
            "nk>private",
            " reasoning</thi",
            "nk>Visible",
            " answer",
        ] {
            let line = format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}",
                serde_json::to_string(content).unwrap()
            );
            process_sse_line(
                line.as_bytes(),
                &mut output,
                Some(&mut filter),
                &progress,
                "Groq",
                "qwen/qwen3.6-27b",
            )
            .unwrap();
        }

        assert_eq!(output, "Visible answer");
        assert_eq!(*snapshots.lock().unwrap(), ["Visible", "Visible answer"]);
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

    #[test]
    fn builds_groq_chat_and_model_endpoints() {
        let profile = ApiProfile {
            provider: GROQ_PROVIDER.into(),
            ..ApiProfile::default()
        };

        assert_eq!(
            chat_completions_url(&profile).unwrap(),
            "https://api.groq.com/openai/v1/chat/completions"
        );
        assert_eq!(
            models_url(&profile).unwrap(),
            "https://api.groq.com/openai/v1/models"
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
