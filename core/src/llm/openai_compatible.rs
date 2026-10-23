use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use reqwest::header;
use serde_json::{json, Value};

use crate::config::ApiProfile;
use crate::providers::{self, OpenAiProtocolBehavior, OPENROUTER_PROVIDER};

use super::http::{completion_error, invalid_response, network_error, response_status_error};
use super::reasoning::{self, InlineReasoningFilter};
use super::{LlmError, LlmProgress, LlmRequest};

#[allow(clippy::too_many_arguments)]
pub(super) async fn generate(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    provider_name: &str,
    behavior: OpenAiProtocolBehavior,
    model_details: Option<&super::openrouter::ModelDetails>,
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
        None,
        model_details,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn generate_with_image(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    image: &[u8],
    provider_name: &str,
    behavior: OpenAiProtocolBehavior,
    model_details: Option<&super::openrouter::ModelDetails>,
) -> Result<String, LlmError> {
    generate_chat_completion(
        http,
        chat_completions_url(profile)?,
        api_key,
        request,
        provider_name,
        behavior,
        Some(profile),
        None,
        Some(image),
        model_details,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn generate_standard(
    http: &reqwest::Client,
    profile: &ApiProfile,
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
        Some(profile),
        on_progress,
        None,
        None,
    )
    .await
}

pub(super) async fn list_models(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
) -> Result<Vec<String>, LlmError> {
    list_model_catalog(http, profile, api_key)
        .await
        .map(|catalog| catalog.models)
}

pub(super) async fn list_model_catalog(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
) -> Result<super::model_catalog::ModelCatalog, LlmError> {
    if profile.provider == OPENROUTER_PROVIDER {
        return super::openrouter::fetch_catalog(http, profile, api_key).await;
    }
    let request = apply_profile_request(http.get(models_url(profile)?), profile, api_key)?;
    super::http::list_model_catalog(request, &profile.provider).await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn test_streaming(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    provider_name: &str,
    behavior: OpenAiProtocolBehavior,
    model_details: Option<&super::openrouter::ModelDetails>,
    on_progress: &LlmProgress,
) -> Result<String, LlmError> {
    let mut body = chat_completion_body(&request, behavior, true);
    if profile.provider == OPENROUTER_PROVIDER {
        super::openrouter::apply_reasoning(&mut body, model_details, request.thinking_enabled);
    }
    super::models_dev::apply_request(&mut body, profile, &request);
    reasoning::apply_explicit_control(
        &mut body,
        &profile.provider,
        &request,
        model_details.or(super::models_dev::model_details(profile, request.model).as_ref()),
    )?;
    let endpoint = chat_completions_url(profile)?;
    check_request_size(&endpoint, &body)?;
    let response = apply_profile_request(http.post(endpoint).json(&body), profile, api_key)?
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
            retry_after: None,
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
    image: Option<&[u8]>,
    model_details: Option<&super::openrouter::ModelDetails>,
) -> Result<String, LlmError> {
    let mut body = chat_completion_body(&request, behavior, on_progress.is_some());
    if profile.is_some_and(|profile| profile.provider == OPENROUTER_PROVIDER) {
        super::openrouter::apply_reasoning(&mut body, model_details, request.thinking_enabled);
    }
    if let Some(profile) = profile {
        super::models_dev::apply_request(&mut body, profile, &request);
        if let Some(details) = model_details {
            details.apply_request(&mut body, &profile.provider, request.thinking_enabled);
        }
        reasoning::apply_explicit_control(
            &mut body,
            &profile.provider,
            &request,
            model_details.or(super::models_dev::model_details(profile, request.model).as_ref()),
        )?;
    }
    if let Some(image) = image {
        body["messages"][1]["content"] = json!([
            { "type": "text", "text": request.input },
            {
                "type": "image_url",
                "image_url": { "url": format!("data:image/png;base64,{}", STANDARD.encode(image)) }
            }
        ]);
    }
    check_request_size(&endpoint, &body)?;
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
    check_completion_status(&value, provider_name)?;
    let text = extract_text(&value).ok_or_else(|| LlmError {
        code: "llm.invalid_response",
        detail: format!("{provider_name} response did not contain text"),
        retryable: false,
        retry_after: None,
    })?;
    let text = reasoning::sanitize_response_text(behavior, request.model, &text);
    if text.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: format!("{provider_name} response did not contain final answer text"),
            retryable: false,
            retry_after: None,
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
    if behavior == OpenAiProtocolBehavior::Mimo {
        body.as_object_mut().unwrap().remove("max_tokens");
        body["max_completion_tokens"] = json!(request.max_output_tokens);
    }
    reasoning::apply_chat_completion_reasoning(
        &mut body,
        behavior,
        request.model,
        request.thinking_enabled,
        stream,
    );
    body
}

pub(super) fn apply_profile_request(
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
                retry_after: None,
            },
        )?;
        let value =
            reqwest::header::HeaderValue::from_str(&custom.value).map_err(|error| LlmError {
                code: "llm.invalid_profile",
                detail: error.to_string(),
                retryable: false,
                retry_after: None,
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
    let mut event = Vec::new();
    let mut finished = false;
    let mut filter = reasoning::should_filter_inline_reasoning(behavior, model)
        .then(InlineReasoningFilter::default);
    while let Some(chunk) = chunks.next().await {
        buffer.extend_from_slice(&chunk.map_err(network_error)?);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line = buffer.drain(..=newline).collect::<Vec<_>>();
            if line.iter().all(|byte| matches!(byte, b'\r' | b'\n')) {
                if process_sse_event(
                    &event,
                    &mut output,
                    filter.as_mut(),
                    on_progress,
                    provider_name,
                    model,
                    &mut finished,
                )? {
                    return completed_filtered_stream(output, filter, on_progress, provider_name);
                }
                event.clear();
            } else {
                event.extend_from_slice(&line);
            }
        }
    }
    event.extend_from_slice(&buffer);
    if !event.is_empty() {
        finished |= process_sse_event(
            &event,
            &mut output,
            filter.as_mut(),
            on_progress,
            provider_name,
            model,
            &mut finished,
        )?;
    }
    if !finished {
        return Err(LlmError {
            code: "llm.provider_unavailable",
            detail: format!("{provider_name} stream ended before completion"),
            retryable: true,
            retry_after: None,
        });
    }
    completed_filtered_stream(output, filter, on_progress, provider_name)
}

#[allow(clippy::too_many_arguments)]
fn process_sse_event(
    event: &[u8],
    output: &mut String,
    filter: Option<&mut InlineReasoningFilter>,
    on_progress: &LlmProgress,
    provider_name: &str,
    model: &str,
    finished: &mut bool,
) -> Result<bool, LlmError> {
    let event = std::str::from_utf8(event).map_err(|error| LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
        retry_after: None,
    })?;
    let data = event
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(false);
    }
    process_sse_line(
        format!("data: {data}").as_bytes(),
        output,
        filter,
        on_progress,
        provider_name,
        model,
        finished,
    )
}

#[allow(clippy::too_many_arguments)]
fn process_sse_line(
    line: &[u8],
    output: &mut String,
    filter: Option<&mut InlineReasoningFilter>,
    on_progress: &LlmProgress,
    provider_name: &str,
    model: &str,
    finished: &mut bool,
) -> Result<bool, LlmError> {
    let line = std::str::from_utf8(line).map_err(|error| LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
        retry_after: None,
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
        retry_after: None,
    })?;
    trace_usage(&value, provider_name, model);
    check_completion_status(&value, provider_name)?;
    *finished |= value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        == Some("stop");
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

fn check_completion_status(value: &Value, provider_name: &str) -> Result<(), LlmError> {
    if let Some(error) = completion_error(value) {
        return Err(error);
    }
    if let Some(reason) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| {
            choices
                .iter()
                .filter_map(|choice| choice.get("finish_reason").and_then(Value::as_str))
                .find(|reason| matches!(*reason, "error" | "content_filter"))
        })
    {
        return Err(LlmError {
            code: "llm.request_failed",
            detail: format!("{provider_name} response failed (finish_reason={reason})"),
            retryable: false,
            retry_after: None,
        });
    }
    if value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices
                .iter()
                .any(|choice| choice.get("finish_reason").and_then(Value::as_str) == Some("length"))
        })
    {
        return Err(LlmError {
            code: "llm.request_failed",
            detail: format!(
                "{provider_name} response was incomplete: output token limit reached (finish_reason=length). Increase the output token limit or disable thinking."
            ),
            retryable: false,
        retry_after: None,
        });
    }
    Ok(())
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
            retry_after: None,
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

fn check_request_size(endpoint: &str, body: &Value) -> Result<(), LlmError> {
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return Ok(());
    };
    if url.path() != "/compatible-mode/v1/chat/completions" {
        return Ok(());
    }
    let host = url.host_str().unwrap_or_default();
    if host == "maas.qianwenaiapi.com" {
        // Qwen AI limits each encoded image to 20 MB and the JSON body to 64 MB.
        // https://platform.qianwenai.com/docs/developer-guides/multimodal/vision
        for content in body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|message| message["content"].as_array().into_iter().flatten())
        {
            if let Some(uri) = content.pointer("/image_url/url").and_then(Value::as_str) {
                if uri.starts_with("data:") && uri.len() > 20_000_000 {
                    return Err(LlmError {
                        code: "llm.request_failed",
                        detail:
                            "Qwen AI image Data URI exceeds the 20 MB limit. Reduce the image size."
                                .into(),
                        retryable: false,
                        retry_after: None,
                    });
                }
            }
        }
        super::http::max_serialized_body(body, 64_000_000, "Qwen AI request (64 MB)")?;
    } else if host.ends_with(".maas.aliyuncs.com") {
        // The 20 MB JSON body cap also bounds every embedded Data URI to 20 MB.
        super::http::max_serialized_body(
            body,
            20_000_000,
            "Alibaba MaaS request and Data URI (20 MB)",
        )?;
    } else if matches!(
        host,
        "dashscope.aliyuncs.com" | "dashscope-intl.aliyuncs.com" | "dashscope-us.aliyuncs.com"
    ) {
        super::http::max_serialized_body(
            body,
            16 * 1024 * 1024,
            "Alibaba DashScope request (16 MiB)",
        )?;
    }
    Ok(())
}

fn chat_completions_url(profile: &ApiProfile) -> Result<String, LlmError> {
    let base_url = if profile.provider == providers::ALIBABA_PROVIDER {
        super::alibaba::base_url(profile)?
    } else {
        providers::effective_base_url(profile).map_err(invalid_profile)?
    };
    let base_url = base_url.trim_end_matches('/');
    if base_url.ends_with("/chat/completions") {
        Ok(base_url.into())
    } else {
        Ok(format!("{base_url}/chat/completions"))
    }
}

pub(super) fn models_url(profile: &ApiProfile) -> Result<String, LlmError> {
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
        retry_after: None,
    }
}

#[cfg(test)]
fn extract_openrouter_models(value: &Value) -> Vec<String> {
    super::openrouter::parse_catalog(value).models
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
    let finish_reason = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .filter(|reason| {
            matches!(
                *reason,
                "stop" | "length" | "error" | "content_filter" | "tool_calls"
            )
        });
    if value.get("usage").is_none() && finish_reason.is_none() {
        return;
    }
    let usage = &value["usage"];
    let generation_id = value.get("id").and_then(Value::as_str).filter(|id| {
        id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    });
    tracing::info!(
        provider = provider_name,
        model,
        finish_reason,
        generation_id,
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

    #[tokio::test]
    async fn openrouter_wire_request_excludes_reasoning_and_preserves_image_budget() {
        use axum::{routing::post, Json, Router};
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let app = Router::new().route("/chat/completions", post(move |Json(body): Json<Value>| async move {
            sender.send(body).unwrap();
            Json(json!({"choices": [{"finish_reason": "stop", "message": {"content": "OK", "reasoning": "private"}}]}))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let profile = ApiProfile {
            provider: OPENROUTER_PROVIDER.into(),
            auth_mode: ApiAuthMode::None,
            ..ApiProfile::default()
        };
        let details = super::super::openrouter::ModelDetails {
            input_modalities: vec!["text".into(), "image".into()],
            reasoning: Some(super::super::openrouter::ReasoningOptions {
                mandatory: Some(true),
                supports_max_tokens: Some(true),
                supported_efforts: Some(vec!["high".into(), "low".into(), "minimal".into()]),
                ..Default::default()
            }),
            field_sources: [("reasoning".into(), "provider_api".into())].into(),
            ..Default::default()
        };
        for (setting, expected_reasoning) in [
            (None, json!({"exclude": true, "effort": "minimal"})),
            (
                Some(crate::config::ReasoningSetting::Effort("high".into())),
                json!({"exclude": true, "effort": "high"}),
            ),
            (
                Some(crate::config::ReasoningSetting::BudgetTokens(128)),
                json!({"exclude": true, "max_tokens": 128}),
            ),
        ] {
            for image in [None, Some(b"png".as_slice())] {
                for streamed in [false, true] {
                    let progress = |_: &str| {};
                    let result = generate_chat_completion(
                        &http,
                        endpoint.clone(),
                        "",
                        LlmRequest {
                            model: "openai/gpt-5-mini",
                            instructions: "Reply",
                            input: "hello",
                            max_output_tokens: 256,
                            thinking_enabled: false,
                            reasoning_control: setting.clone(),
                        },
                        "OpenRouter",
                        OpenAiProtocolBehavior::Standard,
                        Some(&profile),
                        streamed.then_some(&progress as &LlmProgress),
                        image,
                        Some(&details),
                    )
                    .await
                    .unwrap();
                    assert_eq!(result, "OK");
                    let body = receiver.recv().await.unwrap();
                    assert_eq!(body["max_tokens"], 256);
                    assert_eq!(body["reasoning"], expected_reasoning);
                    assert_eq!(body["stream"], streamed);
                    assert!(body.get("enable_thinking").is_none());
                    assert!(body.get("reasoning_effort").is_none());
                    if image.is_some() {
                        assert_eq!(body["messages"][1]["content"][1]["type"], "image_url");
                    }
                }
            }
        }
        server.abort();
    }

    #[tokio::test]
    async fn image_requests_preserve_provider_reasoning_and_final_answer_filtering() {
        use axum::{routing::post, Json, Router};

        for (behavior, model, reply) in [
            (
                OpenAiProtocolBehavior::DeepSeek,
                "deepseek-flash",
                "Visible answer",
            ),
            (
                OpenAiProtocolBehavior::Groq,
                "qwen/qwen3.8-27b",
                "<think>private</think>Visible answer",
            ),
            (
                OpenAiProtocolBehavior::Alibaba,
                "qwen3.8-max",
                "Visible answer",
            ),
            (
                OpenAiProtocolBehavior::Mimo,
                "mimo-v2.6-flash",
                "Visible answer",
            ),
        ] {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let sender = Arc::new(Mutex::new(Some(sender)));
            let app = Router::new().route(
                "/v1/chat/completions",
                post(move |Json(body): Json<Value>| async move {
                    sender.lock().unwrap().take().unwrap().send(body).unwrap();
                    Json(json!({"choices": [{"finish_reason": "stop", "message": {"content": reply, "reasoning_content": "private"}}]}))
                }),
            );
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let profile = ApiProfile {
                provider: OPENAI_COMPATIBLE_PROVIDER.into(),
                base_url: Some(format!("http://{address}/v1")),
                auth_mode: ApiAuthMode::None,
                timeout_ms: 1_000,
                ..ApiProfile::default()
            };
            let request = LlmRequest {
                model,
                instructions: "Answer briefly",
                input: "What is visible?",
                max_output_tokens: 256,
                thinking_enabled: false,
                reasoning_control: None,
            };
            let output = generate_with_image(
                &reqwest::Client::new(),
                &profile,
                "",
                request,
                b"\x89PNG\r\n\x1a\n",
                "Test",
                behavior,
                None,
            )
            .await
            .unwrap();
            let body = receiver.await.unwrap();
            server.abort();
            assert_eq!(output, "Visible answer");
            assert_eq!(body["stream"], false);
            if behavior == OpenAiProtocolBehavior::Mimo {
                assert_eq!(body["max_completion_tokens"], 256);
                assert!(body.get("max_tokens").is_none());
            } else {
                assert_eq!(body["max_tokens"], 256);
            }
            assert_eq!(
                body["messages"][1]["content"],
                json!([
                    {"type": "text", "text": "What is visible?"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}}
                ])
            );
            match behavior {
                OpenAiProtocolBehavior::DeepSeek | OpenAiProtocolBehavior::Mimo => {
                    assert_eq!(body["thinking"]["type"], "disabled")
                }
                OpenAiProtocolBehavior::Groq => {
                    assert_eq!(body["reasoning_format"], "hidden");
                    assert_eq!(body["reasoning_effort"], "none");
                }
                OpenAiProtocolBehavior::Alibaba => assert_eq!(body["enable_thinking"], false),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn image_chat_endpoints_follow_provider_connections() {
        for (provider, region, workspace, expected) in [
            (providers::DEEPSEEK_PROVIDER, None, None, "https://api.deepseek.com/v1/chat/completions"),
            (GROQ_PROVIDER, None, None, "https://api.groq.com/openai/v1/chat/completions"),
            (providers::QWEN_AI_PROVIDER, None, None, "https://maas.qianwenaiapi.com/compatible-mode/v1/chat/completions"),
            (providers::ALIBABA_PROVIDER, Some("singapore"), None, "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions"),
            (providers::ALIBABA_PROVIDER, Some("china_beijing"), Some("ws-example"), "https://ws-example.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/chat/completions"),
        ] {
            let profile = ApiProfile {
                provider: provider.into(),
                region: region.map(str::to_owned),
                workspace_id: workspace.map(str::to_owned),
                ..ApiProfile::default()
            };
            assert_eq!(chat_completions_url(&profile).unwrap(), expected);
        }
    }

    #[test]
    fn alibaba_limits_use_the_serialized_body_and_confirmed_endpoint() {
        let mut body = json!({"messages": [{"content": [
            {"type": "text", "text": ""},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,cG5n"}}
        ]}]});
        let overhead = serde_json::to_vec(&body).unwrap().len();
        for (endpoint, limit, name) in [
            (
                "https://ws.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/chat/completions",
                20_000_000,
                "20 MB",
            ),
            (
                "https://ws.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1/chat/completions",
                20_000_000,
                "20 MB",
            ),
            (
                "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
                16 * 1024 * 1024,
                "16 MiB",
            ),
            (
                "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions",
                16 * 1024 * 1024,
                "16 MiB",
            ),
        ] {
            body["messages"][0]["content"][0]["text"] = json!("a".repeat(limit - overhead));
            check_request_size(endpoint, &body).unwrap();

            body["messages"][0]["content"][0]["text"] = json!("a".repeat(limit - overhead + 1));
            let error = check_request_size(endpoint, &body).unwrap_err();
            assert!(error.detail.contains(name));
            assert!(error.detail.contains("serialized JSON body limit"));
            assert!(!error.retryable);
        }
        body["messages"][0]["content"][0]["text"] = json!("");
        body["messages"][0]["content"][1]["image_url"]["url"] =
            json!(format!("data:image/png;base64,{}", "A".repeat(20_000_000)));
        let error = check_request_size(
            "https://ws.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/chat/completions",
            &body,
        )
        .unwrap_err();
        assert!(error.detail.contains("Data URI (20 MB)"));
        assert!(!error.retryable);
        for endpoint in [
            "https://api.openai.com/v1/chat/completions",
            "https://api.groq.com/openai/v1/chat/completions",
            "https://api.deepseek.com/v1/chat/completions",
            "https://custom.example/compatible-mode/v1/chat/completions",
            "https://ws.cn-beijing.maas.aliyuncs.com.example/compatible-mode/v1/chat/completions",
            "https://custom-dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
            "https://dashscope.aliyuncs.com/other/chat/completions",
        ] {
            check_request_size(endpoint, &body).unwrap();
        }
    }

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

    #[tokio::test]
    async fn qwen_ai_image_requests_preserve_auth_image_and_thinking_controls() {
        use axum::{http::HeaderMap, routing::post, Json, Router};
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let app = Router::new().route(
            "/chat/completions",
            post(
                move |headers: HeaderMap, Json(body): Json<Value>| async move {
                    sender.send((headers, body)).unwrap();
                    Json(json!({"choices": [{"finish_reason": "stop", "message": {
                        "content": "A red square", "reasoning_content": "private reasoning"
                    }}]}))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let profile = ApiProfile {
            provider: providers::QWEN_AI_PROVIDER.into(),
            enabled_capabilities: vec![providers::CAPABILITY_IMAGE_UNDERSTANDING.into()],
            ..Default::default()
        };
        for model in ["qwen3.8-max", "qwen3.8-flash", "qwen3-vl-plus"] {
            for enabled in [false, true] {
                let request = LlmRequest {
                    model,
                    instructions: "Answer briefly",
                    input: "What is visible?",
                    max_output_tokens: 256,
                    thinking_enabled: enabled,
                    reasoning_control: None,
                };
                let answer = generate_chat_completion(
                    &http,
                    endpoint.clone(),
                    "fixture-key",
                    request,
                    "Qwen AI",
                    OpenAiProtocolBehavior::Alibaba,
                    Some(&profile),
                    None,
                    Some(b"\x89PNG\r\n\x1a\n"),
                    None,
                )
                .await
                .unwrap();
                let (headers, body) = receiver.recv().await.unwrap();
                assert_eq!(headers["authorization"], "Bearer fixture-key");
                assert_eq!(body["model"], model);
                assert_eq!(body["enable_thinking"], enabled, "{model}");
                assert_eq!(body["max_tokens"], 256);
                assert_eq!(body["stream"], false);
                assert_eq!(
                    body["messages"][1]["content"],
                    json!([
                        {"type": "text", "text": "What is visible?"},
                        {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}}
                    ])
                );
                assert_eq!(answer, "A red square");
            }
        }
        server.abort();
    }

    #[test]
    fn qwen_ai_bounds_each_data_uri_and_the_total_json_body() {
        let endpoint = "https://maas.qianwenaiapi.com/compatible-mode/v1/chat/completions";
        let mut body = json!({"messages": [{"content": [{
            "type": "image_url", "image_url": {"url": ""}
        }]}]});
        let prefix = "data:image/png;base64,";
        body["messages"][0]["content"][0]["image_url"]["url"] =
            json!(format!("{prefix}{}", "A".repeat(20_000_000 - prefix.len())));
        check_request_size(endpoint, &body).unwrap();
        body["messages"][0]["content"][0]["image_url"]["url"] = json!(format!(
            "{prefix}{}",
            "A".repeat(20_000_000 - prefix.len() + 1)
        ));
        let error = check_request_size(endpoint, &body).unwrap_err();
        assert_eq!(error.code, "llm.request_failed");
        assert!(!error.retryable);
        assert!(error.detail.contains("Data URI"));
        check_request_size(
            "https://maas.qianwenaiapi.com.example/compatible-mode/v1/chat/completions",
            &body,
        )
        .unwrap();

        let mut body = json!({"messages": [{"content": ""}]});
        let overhead = serde_json::to_vec(&body).unwrap().len();
        body["messages"][0]["content"] = json!("a".repeat(64_000_000 - overhead));
        check_request_size(endpoint, &body).unwrap();
        body["messages"][0]["content"] = json!("a".repeat(64_000_000 - overhead + 1));
        let error = check_request_size(endpoint, &body).unwrap_err();
        assert_eq!(error.code, "llm.request_failed");
        assert!(!error.retryable);
        assert!(error.detail.contains("serialized JSON body limit"));
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
            reasoning_control: None,
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
    fn mimo_body_controls_thinking_and_uses_completion_token_budget() {
        let profile = ApiProfile {
            provider: "mimo".into(),
            enabled_capabilities: vec![providers::CAPABILITY_TEXT_GENERATION.into()],
            ..Default::default()
        };
        let service =
            providers::resolve_profile_capability(&profile, providers::CAPABILITY_TEXT_GENERATION)
                .unwrap();
        let providers::ServiceAdapter::OpenAiChatCompletions { behavior } = service.service.adapter
        else {
            panic!("MiMo must use Chat Completions");
        };
        for enabled in [false, true] {
            let request = LlmRequest {
                model: "mimo-v2.6-flash",
                instructions: "Translate",
                input: "hello",
                max_output_tokens: 128,
                thinking_enabled: enabled,
                reasoning_control: None,
            };
            let body = chat_completion_body(&request, behavior, enabled);
            assert_eq!(body["max_completion_tokens"], 128);
            assert!(body.get("max_tokens").is_none());
            assert_eq!(
                body["thinking"]["type"],
                if enabled { "enabled" } else { "disabled" }
            );
            assert_eq!(body["messages"][0]["content"], "Translate");
            assert_eq!(body["stream"], enabled);
            assert!(body.get("temperature").is_none());
            assert!(body.get("reasoning_effort").is_none());
        }
    }

    #[test]
    fn maps_groq_reasoning_models_without_conflicting_parameters() {
        let request = |model, thinking_enabled| LlmRequest {
            model,
            instructions: "Translate",
            input: "hello",
            max_output_tokens: 128,
            thinking_enabled,
            reasoning_control: None,
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
        let vision_enabled = chat_completion_body(
            &request("qwen/qwen3.8-27b", true),
            OpenAiProtocolBehavior::Groq,
            false,
        );
        assert_eq!(vision_enabled["reasoning_format"], "hidden");
        assert_eq!(vision_enabled["reasoning_effort"], "medium");
        assert!(vision_enabled.get("include_reasoning").is_none());
    }

    #[test]
    fn only_alibaba_hybrid_models_receive_enable_thinking() {
        let request = |model| LlmRequest {
            model,
            instructions: "Translate",
            input: "hello",
            max_output_tokens: 128,
            thinking_enabled: false,
            reasoning_control: None,
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
    fn alibaba_v41_thinking_toggle_is_sent_in_the_request() {
        for model in [
            "deepseek-v4.1-flash",
            "vanchin/deepseek-v4-flash",
            "vanchin/deepseek-v4.1-flash",
        ] {
            for enabled in [false, true] {
                let body = chat_completion_body(
                    &LlmRequest {
                        model,
                        instructions: "Translate",
                        input: "hello",
                        max_output_tokens: 4096,
                        thinking_enabled: enabled,
                        reasoning_control: None,
                    },
                    OpenAiProtocolBehavior::Alibaba,
                    true,
                );
                assert_eq!(
                    body.get("enable_thinking"),
                    Some(&json!(enabled)),
                    "{model}"
                );
            }
        }
    }

    async fn chat_fixture_result(
        body: String,
        streaming: bool,
        behavior: OpenAiProtocolBehavior,
    ) -> Result<String, LlmError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let content_type = if streaming {
            "text/event-stream"
        } else {
            "application/json"
        };
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move || async move {
                ([(axum::http::header::CONTENT_TYPE, content_type)], body)
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let progress = |_: &str| {};
        let result = generate_standard(
            &client,
            &ApiProfile::default(),
            endpoint,
            "fixture-key",
            LlmRequest {
                model: "deepseek-v4.1-flash",
                instructions: "Translate",
                input: "hello",
                max_output_tokens: 128,
                thinking_enabled: true,
                reasoning_control: None,
            },
            "Test provider",
            behavior,
            streaming.then_some(&progress as &LlmProgress),
        )
        .await;
        server.abort();
        result
    }

    #[tokio::test]
    async fn provider_errors_never_accept_partial_output() {
        let error = json!({"code": 429, "message": "Rate limited", "metadata": {"error_type": "rate_limit_exceeded"}});
        for (body, streaming) in [
            (json!({"error": error}).to_string(), false),
            (json!({"choices": [{"message": {"content": "Partial"}, "finish_reason": "error", "error": error}]}).to_string(), false),
            (format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", json!({"choices": [{"delta": {"content": "Partial"}}]}), json!({"error": error, "choices": [{"delta": {}, "finish_reason": "error"}]})), true),
        ] {
            let failure = chat_fixture_result(body, streaming, OpenAiProtocolBehavior::Standard).await.unwrap_err();
            assert_eq!(failure.code, "llm.rate_limited");
            assert!(failure.retryable);
        }
    }

    #[tokio::test]
    async fn interrupted_streams_do_not_accept_partial_output() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Partial\"}}]}\n\n";
        let error = chat_fixture_result(body.into(), true, OpenAiProtocolBehavior::Standard)
            .await
            .unwrap_err();
        assert_eq!(error.code, "llm.provider_unavailable");
    }

    #[tokio::test]
    async fn multiline_events_and_heartbeat_preserve_text() {
        let body = ": OPENROUTER PROCESSING\r\n\r\ndata: {\"choices\":\r\ndata: [{\"delta\":{\"content\":\"Hello\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\ndata: [DONE]\r\n\r\n";
        assert_eq!(
            chat_fixture_result(body.into(), true, OpenAiProtocolBehavior::Standard)
                .await
                .unwrap(),
            "Hello"
        );
    }

    #[tokio::test]
    async fn terminal_stop_and_trailing_usage_complete_without_done_marker() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"completion_tokens\":2}}\n\n";
        assert_eq!(
            chat_fixture_result(body.into(), true, OpenAiProtocolBehavior::Standard)
                .await
                .unwrap(),
            "你好"
        );
    }

    #[test]
    fn failed_finish_reasons_without_error_objects_are_rejected() {
        for reason in ["error", "content_filter"] {
            assert!(check_completion_status(
                &json!({"choices": [{"finish_reason": reason}]}),
                "Test"
            )
            .is_err());
        }
    }

    #[tokio::test]
    async fn mimo_streaming_returns_only_visible_text() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        assert_eq!(
            chat_fixture_result(body.into(), true, OpenAiProtocolBehavior::Mimo)
                .await
                .unwrap(),
            "Hello"
        );
    }

    #[tokio::test]
    async fn truncated_json_reports_output_exhaustion_for_every_chat_behavior() {
        for behavior in [
            OpenAiProtocolBehavior::Standard,
            OpenAiProtocolBehavior::Alibaba,
            OpenAiProtocolBehavior::DeepSeek,
            OpenAiProtocolBehavior::Groq,
            OpenAiProtocolBehavior::Mimo,
        ] {
            for content in ["", "Partial translation"] {
                let body = json!({ "choices": [{
                    "finish_reason": "length",
                    "message": { "content": content, "reasoning_content": "Private reasoning" }
                }] })
                .to_string();
                let error = chat_fixture_result(body, false, behavior)
                    .await
                    .unwrap_err();
                assert_eq!(error.code, "llm.request_failed");
                assert!(error.detail.contains("finish_reason=length"));
                assert!(!error.retryable);
                assert!(!error.detail.contains("Private reasoning"));
            }
            let body = json!({ "choices": [{
                "finish_reason": "stop", "message": { "content": "Hello" }
            }] })
            .to_string();
            assert_eq!(
                chat_fixture_result(body, false, behavior).await.unwrap(),
                "Hello"
            );
        }
    }

    #[tokio::test]
    async fn truncated_sse_reports_output_exhaustion_for_every_chat_behavior() {
        for behavior in [
            OpenAiProtocolBehavior::Standard,
            OpenAiProtocolBehavior::Alibaba,
            OpenAiProtocolBehavior::DeepSeek,
            OpenAiProtocolBehavior::Groq,
            OpenAiProtocolBehavior::Mimo,
        ] {
            for content in ["", "Partial translation"] {
                let body = format!(
                    "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                    json!({ "choices": [{ "delta": { "content": content, "reasoning_content": "Private reasoning" } }] }),
                    json!({ "choices": [{ "delta": {}, "finish_reason": "length" }] }),
                );
                let error = chat_fixture_result(body, true, behavior).await.unwrap_err();
                assert_eq!(error.code, "llm.request_failed");
                assert!(error.detail.contains("finish_reason=length"));
                assert!(!error.retryable);
            }
            let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            assert_eq!(
                chat_fixture_result(body.into(), true, behavior)
                    .await
                    .unwrap(),
                "Hello"
            );
        }
    }

    #[tokio::test]
    async fn rejects_length_in_the_last_sse_line_without_a_newline() {
        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Partial\"}}]}\n\n",
                "data: {\"choices\":[{\"finish_reason\":\"length\",\"delta\":{}}]}"
            ))
            .unwrap()
            .into();
        let error = stream_chat_completion(
            response,
            &|_| {},
            "Test",
            OpenAiProtocolBehavior::Standard,
            "test-model",
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "llm.request_failed");
        assert!(error.detail.contains("finish_reason=length"));
        assert!(!error.retryable);
    }

    #[test]
    fn checks_length_in_all_choices() {
        let error = check_completion_status(
            &json!({"choices": [
                {"finish_reason": "stop"}, {"finish_reason": "length"}
            ]}),
            "Test",
        )
        .unwrap_err();
        assert!(error.detail.contains("finish_reason=length"));
        assert!(!error.retryable);
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
            &mut false,
        )
        .unwrap());
        assert!(!process_sse_line(
            br#"data: {"choices":[{"delta":{"content":"\u597d"}}]}"#,
            &mut output,
            None,
            &progress,
            "DeepSeek",
            "deepseek-v4-flash",
            &mut false,
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
            &mut false,
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
                &mut false,
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
