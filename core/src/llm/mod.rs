//! 通用 LLM HTTP 客户端。翻译、语法分析等功能只负责构造任务，
//! 不直接依赖供应商协议。

use std::time::Instant;

use futures_util::StreamExt;
use reqwest::{header, StatusCode};
use serde_json::{json, Value};

use crate::config::{ApiProfile, ALIBABA_PROVIDER, OPENAI_PROVIDER};

#[derive(Debug, Clone)]
pub struct LlmRequest<'a> {
    pub model: &'a str,
    pub instructions: &'a str,
    pub input: &'a str,
    pub max_output_tokens: u32,
    pub thinking_enabled: bool,
}

pub type LlmProgress = dyn Fn(&str) + Send + Sync;

#[derive(Debug, Clone, PartialEq)]
pub struct LlmError {
    pub code: &'static str,
    pub detail: String,
    pub retryable: bool,
}

#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    pub async fn generate(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        request: LlmRequest<'_>,
        on_progress: Option<&LlmProgress>,
    ) -> Result<String, LlmError> {
        let started = Instant::now();
        let model = request.model.to_owned();
        let input_chars = request.input.chars().count();
        let thinking_enabled = request.thinking_enabled;
        let result = match profile.provider.as_str() {
            OPENAI_PROVIDER => self.openai(profile, api_key, request, on_progress).await,
            ALIBABA_PROVIDER => self.alibaba(profile, api_key, request, on_progress).await,
            provider => Err(LlmError {
                code: "llm.unsupported_provider",
                detail: format!("Unsupported LLM provider: {provider}"),
                retryable: false,
            }),
        };
        tracing::info!(
            provider = profile.provider,
            model,
            latency_ms = started.elapsed().as_millis() as u64,
            input_chars,
            output_chars = result.as_ref().map_or(0, |text| text.chars().count()),
            thinking_enabled,
            streamed = on_progress.is_some(),
            success = result.is_ok(),
            "LLM request completed"
        );
        result
    }

    pub async fn list_models(
        &self,
        profile: &ApiProfile,
        api_key: &str,
    ) -> Result<Vec<String>, LlmError> {
        let endpoint = match profile.provider.as_str() {
            OPENAI_PROVIDER => openai_models_url(profile)?,
            ALIBABA_PROVIDER => format!("{}/models", alibaba_base_url(profile)?),
            provider => {
                return Err(LlmError {
                    code: "llm.models_unsupported",
                    detail: format!("Provider {provider} does not expose LLM models"),
                    retryable: false,
                })
            }
        };
        let response = self
            .http
            .get(endpoint)
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(network_error)?;
        let status = response.status();
        let value: Value = response.json().await.map_err(invalid_response)?;
        if !status.is_success() {
            return Err(status_error(status, &value));
        }
        let models = extract_model_ids(&value);
        if models.is_empty() {
            return Err(LlmError {
                code: "llm.invalid_response",
                detail: "The LLM service did not return any models".into(),
                retryable: false,
            });
        }
        Ok(models)
    }

    async fn openai(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        request: LlmRequest<'_>,
        on_progress: Option<&LlmProgress>,
    ) -> Result<String, LlmError> {
        if profile.uses_openai_compatible_api() {
            let deepseek_protocol = uses_deepseek_protocol(profile, request.model);
            return self
                .chat_completions(
                    openai_chat_completions_url(profile)?,
                    api_key,
                    request,
                    "OpenAI-compatible",
                    deepseek_protocol,
                    on_progress,
                )
                .await;
        }
        let response = self
            .http
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(api_key)
            .json(&json!({
                "model": request.model,
                "instructions": request.instructions,
                "input": request.input,
                "max_output_tokens": request.max_output_tokens
            }))
            .send()
            .await
            .map_err(network_error)?;
        let status = response.status();
        let value: Value = response.json().await.map_err(invalid_response)?;
        if !status.is_success() {
            return Err(status_error(status, &value));
        }
        extract_openai_text(&value).ok_or_else(|| LlmError {
            code: "llm.invalid_response",
            detail: "OpenAI response did not contain text".into(),
            retryable: false,
        })
    }

    async fn alibaba(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        request: LlmRequest<'_>,
        on_progress: Option<&LlmProgress>,
    ) -> Result<String, LlmError> {
        let endpoint = alibaba_base_url(profile)?;
        self.chat_completions(
            format!("{endpoint}/chat/completions"),
            api_key,
            request,
            "Alibaba Cloud",
            false,
            on_progress,
        )
        .await
    }

    async fn chat_completions(
        &self,
        endpoint: String,
        api_key: &str,
        request: LlmRequest<'_>,
        provider_name: &str,
        deepseek_protocol: bool,
        on_progress: Option<&LlmProgress>,
    ) -> Result<String, LlmError> {
        let body = chat_completion_body(&request, deepseek_protocol, on_progress.is_some());
        let response = self
            .http
            .post(endpoint)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .map_err(network_error)?;
        let status = response.status();
        if !status.is_success() {
            let value: Value = response.json().await.map_err(invalid_response)?;
            return Err(status_error(status, &value));
        }
        let is_event_stream = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));
        if let Some(progress) = on_progress.filter(|_| is_event_stream) {
            return stream_chat_completion(response, progress, provider_name, request.model).await;
        }
        let value: Value = response.json().await.map_err(invalid_response)?;
        trace_usage(&value, provider_name, request.model);
        let text = extract_chat_completion_text(&value).ok_or_else(|| LlmError {
            code: "llm.invalid_response",
            detail: format!("{provider_name} response did not contain text"),
            retryable: false,
        })?;
        if let Some(progress) = on_progress {
            progress(&text);
        }
        Ok(text)
    }
}

fn chat_completion_body(request: &LlmRequest<'_>, deepseek_protocol: bool, stream: bool) -> Value {
    let mut body = json!({
        "model": request.model,
        "messages": [
            { "role": "system", "content": request.instructions },
            { "role": "user", "content": request.input }
        ],
        "max_tokens": request.max_output_tokens,
        "stream": stream
    });
    if deepseek_protocol {
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

fn uses_deepseek_protocol(profile: &ApiProfile, model: &str) -> bool {
    model.to_ascii_lowercase().starts_with("deepseek-")
        || profile
            .base_url
            .as_deref()
            .is_some_and(|url| url.to_ascii_lowercase().contains("deepseek"))
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
    if let Some(delta) = extract_chat_completion_delta(&value) {
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

fn openai_chat_completions_url(profile: &ApiProfile) -> Result<String, LlmError> {
    let base_url = profile
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| LlmError {
            code: "llm.invalid_profile",
            detail: "OpenAI-compatible Base URL is missing".into(),
            retryable: false,
        })?
        .trim_end_matches('/');
    if base_url.ends_with("/chat/completions") {
        Ok(base_url.into())
    } else {
        Ok(format!("{base_url}/chat/completions"))
    }
}

fn openai_models_url(profile: &ApiProfile) -> Result<String, LlmError> {
    if !profile.uses_openai_compatible_api() {
        return Ok("https://api.openai.com/v1/models".into());
    }
    let base_url = profile
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| LlmError {
            code: "llm.invalid_profile",
            detail: "OpenAI-compatible Base URL is missing".into(),
            retryable: false,
        })?
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

fn alibaba_base_url(profile: &ApiProfile) -> Result<String, LlmError> {
    let endpoint = match profile.region.as_deref() {
        Some("china_beijing") => "https://dashscope.aliyuncs.com/compatible-mode/v1",
        Some("singapore") => "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        _ => {
            return Err(LlmError {
                code: "llm.invalid_profile",
                detail: "Alibaba Cloud region is invalid".into(),
                retryable: false,
            })
        }
    };
    Ok(endpoint.into())
}

fn extract_openai_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Some(text.trim().to_owned()).filter(|text| !text.is_empty());
    }
    value
        .get("output")?
        .as_array()?
        .iter()
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find_map(|content| content.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn extract_chat_completion_text(value: &Value) -> Option<String> {
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

fn extract_chat_completion_delta(value: &Value) -> Option<String> {
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

fn extract_model_ids(value: &Value) -> Vec<String> {
    let entries = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array);
    let mut models = entries
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            entry
                .get("id")
                .or_else(|| entry.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|model| !model.is_empty() && model.chars().count() <= 200)
                .map(str::to_owned)
        })
        .take(500)
        .collect::<Vec<_>>();
    models.sort_by_key(|model| model.to_ascii_lowercase());
    models.dedup();
    models
}

fn network_error(error: reqwest::Error) -> LlmError {
    LlmError {
        code: if error.is_timeout() {
            "llm.timeout"
        } else {
            "llm.network_failed"
        },
        detail: error.to_string(),
        retryable: true,
    }
}

fn invalid_response(error: reqwest::Error) -> LlmError {
    LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
    }
}

fn status_error(status: StatusCode, value: &Value) -> LlmError {
    let detail = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("LLM request failed")
        .to_owned();
    let (code, retryable) = match status.as_u16() {
        401 | 403 => ("llm.authentication_failed", false),
        408 => ("llm.timeout", true),
        429 => ("llm.rate_limited", true),
        500..=599 => ("llm.provider_unavailable", true),
        _ => ("llm.request_failed", false),
    };
    LlmError {
        code,
        detail,
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn extracts_responses_api_text() {
        let value = json!({
            "output": [{"content": [{"type": "output_text", "text": " hello "}]}]
        });
        assert_eq!(extract_openai_text(&value).as_deref(), Some("hello"));
    }

    #[test]
    fn extracts_chat_completions_text() {
        let value = json!({
            "choices": [{"message": {"role": "assistant", "content": " hello "}}]
        });
        assert_eq!(
            extract_chat_completion_text(&value).as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn deepseek_translation_body_disables_thinking_and_bounds_output() {
        let request = LlmRequest {
            model: "deepseek-v4-flash",
            instructions: "Translate",
            input: "hello",
            max_output_tokens: 128,
            thinking_enabled: false,
        };
        let body = chat_completion_body(&request, true, true);
        assert_eq!(body.pointer("/thinking/type"), Some(&json!("disabled")));
        assert_eq!(body.get("temperature"), Some(&json!(0)));
        assert_eq!(body.get("max_tokens"), Some(&json!(128)));
        assert_eq!(body.get("stream"), Some(&json!(true)));

        let generic = chat_completion_body(&request, false, false);
        assert!(generic.get("thinking").is_none());
        assert!(generic.get("temperature").is_none());
    }

    #[test]
    fn accumulates_streamed_chat_completion_content() {
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
    fn builds_openai_compatible_chat_completions_endpoint() {
        let profile = ApiProfile {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            provider: OPENAI_PROVIDER.into(),
            region: None,
            workspace_id: None,
            base_url: Some("https://api.deepseek.com/v1/".into()),
            purpose: None,
        };
        assert_eq!(
            openai_chat_completions_url(&profile).unwrap(),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn builds_models_endpoint_from_base_or_full_chat_endpoint() {
        let mut profile = ApiProfile {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            provider: OPENAI_PROVIDER.into(),
            region: None,
            workspace_id: None,
            base_url: Some("https://api.deepseek.com/v1".into()),
            purpose: None,
        };
        assert_eq!(
            openai_models_url(&profile).unwrap(),
            "https://api.deepseek.com/v1/models"
        );
        profile.base_url = Some("https://example.com/v1/chat/completions".into());
        assert_eq!(
            openai_models_url(&profile).unwrap(),
            "https://example.com/v1/models"
        );
    }

    #[test]
    fn extracts_and_normalizes_model_ids() {
        let value = json!({
            "data": [
                {"id": "deepseek-reasoner"},
                {"id": " deepseek-chat "},
                {"id": "deepseek-chat"},
                {"id": ""}
            ]
        });
        assert_eq!(
            extract_model_ids(&value),
            vec!["deepseek-chat", "deepseek-reasoner"]
        );
    }

    #[test]
    fn builds_region_specific_alibaba_endpoint() {
        let profile = ApiProfile {
            id: "one".into(),
            name: "One".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("singapore".into()),
            workspace_id: Some("ws-example".into()),
            base_url: None,
            purpose: None,
        };
        assert_eq!(
            alibaba_base_url(&profile).unwrap(),
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        );
    }
}
