use std::time::Duration;

use serde_json::{json, Value};

use crate::config::ApiProfile;

use super::http::{invalid_response, list_models as parse_models, network_error, status_error};
use super::{LlmError, LlmRequest};

const MAX_RETRY_OUTPUT_TOKENS: u32 = 16_384;

pub(super) async fn generate(
    http: &reqwest::Client,
    api_key: &str,
    request: LlmRequest<'_>,
) -> Result<String, LlmError> {
    let mut max_output_tokens = request.max_output_tokens;
    for attempt in 0..=1 {
        let response = http
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(api_key)
            .json(&request_body(&request, max_output_tokens))
            .send()
            .await
            .map_err(network_error)?;
        let status = response.status();
        let value: Value = response.json().await.map_err(invalid_response)?;
        if !status.is_success() {
            return Err(status_error(status, &value));
        }
        match parse_response(&value)? {
            ParsedResponse::Complete(text) => return Ok(text),
            ParsedResponse::Incomplete(reason) => {
                if attempt == 0 {
                    if let Some(retry_limit) = retry_output_token_limit(&reason, max_output_tokens)
                    {
                        tracing::warn!(
                            model = request.model,
                            initial_limit = max_output_tokens,
                            retry_limit,
                            "Retrying incomplete OpenAI response with a larger output budget"
                        );
                        max_output_tokens = retry_limit;
                        continue;
                    }
                }
                return Err(incomplete_error(&reason));
            }
        }
    }
    unreachable!("OpenAI response loop returns after at most one retry")
}

fn request_body(request: &LlmRequest<'_>, max_output_tokens: u32) -> Value {
    let mut body = json!({
        "model": request.model,
        "instructions": request.instructions,
        "input": request.input,
        "max_output_tokens": max_output_tokens
    });
    if let Some(effort) = reasoning_effort(request.model, request.thinking_enabled) {
        body["reasoning"] = json!({ "effort": effort });
    }
    body
}

fn reasoning_effort(model: &str, enabled: bool) -> Option<&'static str> {
    const MINIMAL_MODELS: &[&str] = &["gpt-5", "gpt-5-mini", "gpt-5-nano"];
    const NONE_MODELS: &[&str] = &[
        "gpt-5.1",
        "gpt-5.2",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.4-nano",
        "gpt-5.5",
        "gpt-5.6",
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
    ];
    let model = model.trim().to_ascii_lowercase();
    if MINIMAL_MODELS
        .iter()
        .any(|family| is_model_or_snapshot(&model, family))
    {
        return Some(if enabled { "medium" } else { "minimal" });
    }
    if NONE_MODELS
        .iter()
        .any(|family| is_model_or_snapshot(&model, family))
    {
        return Some(if enabled { "medium" } else { "none" });
    }
    None
}

fn is_model_or_snapshot(model: &str, family: &str) -> bool {
    model == family
        || model
            .strip_prefix(family)
            .is_some_and(|suffix| suffix.starts_with("-20"))
}

fn retry_output_token_limit(reason: &str, current: u32) -> Option<u32> {
    if reason != "max_output_tokens" {
        return None;
    }
    let next = current.saturating_mul(2).min(MAX_RETRY_OUTPUT_TOKENS);
    (next > current).then_some(next)
}

enum ParsedResponse {
    Complete(String),
    Incomplete(String),
}

fn incomplete_error(reason: &str) -> LlmError {
    LlmError {
        code: "llm.request_failed",
        detail: format!("OpenAI response was incomplete: {reason}"),
        retryable: false,
    }
}

fn parse_response(value: &Value) -> Result<ParsedResponse, LlmError> {
    if value.get("status").and_then(Value::as_str) == Some("incomplete") {
        let reason = value
            .pointer("/incomplete_details/reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown reason");
        return Ok(ParsedResponse::Incomplete(reason.to_owned()));
    }
    extract_text(value)
        .map(ParsedResponse::Complete)
        .ok_or_else(|| LlmError {
            code: "llm.invalid_response",
            detail: "OpenAI response did not contain text".into(),
            retryable: false,
        })
}

pub(super) async fn list_models(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
) -> Result<Vec<String>, LlmError> {
    parse_models(
        http.get("https://api.openai.com/v1/models")
            .timeout(Duration::from_millis(profile.timeout_ms))
            .bearer_auth(api_key),
    )
    .await
}

fn extract_text(value: &Value) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_responses_api_text() {
        let value = json!({
            "output": [{"content": [{"type": "output_text", "text": " hello "}]}]
        });
        assert_eq!(extract_text(&value).as_deref(), Some("hello"));
    }

    #[test]
    fn builds_model_compatible_reasoning_controls() {
        let request = |model, thinking_enabled| LlmRequest {
            model,
            instructions: "Translate",
            input: "hello",
            max_output_tokens: 1_024,
            thinking_enabled,
        };

        assert_eq!(
            request_body(&request("gpt-5-mini", false), 1_024)
                .pointer("/reasoning/effort")
                .and_then(Value::as_str),
            Some("minimal")
        );
        assert_eq!(
            request_body(&request("gpt-5.4-mini", false), 1_024)
                .pointer("/reasoning/effort")
                .and_then(Value::as_str),
            Some("none")
        );
        assert_eq!(
            request_body(&request("gpt-5-mini", true), 1_024)
                .pointer("/reasoning/effort")
                .and_then(Value::as_str),
            Some("medium")
        );
        assert!(request_body(&request("gpt-4o-mini", false), 1_024)
            .get("reasoning")
            .is_none());
    }

    #[test]
    fn limits_output_budget_retry_to_one_bounded_increase() {
        assert_eq!(
            retry_output_token_limit("max_output_tokens", 1_024),
            Some(2_048)
        );
        assert_eq!(
            retry_output_token_limit("max_output_tokens", 10_000),
            Some(16_384)
        );
        assert_eq!(retry_output_token_limit("max_output_tokens", 16_384), None);
        assert_eq!(retry_output_token_limit("content_filter", 1_024), None);
    }

    #[test]
    fn classifies_incomplete_responses_without_accepting_partial_text() {
        let value = json!({
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "output_text": "partial"
        });

        let ParsedResponse::Incomplete(reason) = parse_response(&value).unwrap() else {
            panic!("expected an incomplete response");
        };
        assert_eq!(reason, "max_output_tokens");
        let error = incomplete_error(&reason);
        assert_eq!(error.code, "llm.request_failed");
        assert_eq!(
            error.detail,
            "OpenAI response was incomplete: max_output_tokens"
        );
        assert!(!error.retryable);
    }
}
