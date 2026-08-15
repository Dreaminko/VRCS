use std::time::Duration;

use serde_json::{json, Value};

use crate::config::ApiProfile;

use super::http::{invalid_response, list_models as parse_models, network_error, status_error};
use super::{LlmError, LlmRequest};

pub(super) async fn generate(
    http: &reqwest::Client,
    api_key: &str,
    request: LlmRequest<'_>,
) -> Result<String, LlmError> {
    let response = http
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
    parse_response(&value)
}

fn parse_response(value: &Value) -> Result<String, LlmError> {
    if value.get("status").and_then(Value::as_str) == Some("incomplete") {
        let reason = value
            .pointer("/incomplete_details/reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown reason");
        return Err(LlmError {
            code: "llm.request_failed",
            detail: format!("OpenAI response was incomplete: {reason}"),
            retryable: false,
        });
    }
    extract_text(value).ok_or_else(|| LlmError {
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
    fn reports_incomplete_responses_without_accepting_partial_text() {
        let value = json!({
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "output_text": "partial"
        });

        let error = parse_response(&value).unwrap_err();

        assert_eq!(error.code, "llm.request_failed");
        assert_eq!(
            error.detail,
            "OpenAI response was incomplete: max_output_tokens"
        );
        assert!(!error.retryable);
    }
}
