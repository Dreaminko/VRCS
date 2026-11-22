use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

use crate::config::ApiProfile;

use super::http::{
    invalid_response, list_model_catalog as parse_catalog, network_error, response_status_error,
};
use super::{LlmError, LlmRequest};

const MAX_RETRY_OUTPUT_TOKENS: u32 = 16_384;

pub(super) async fn generate(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
) -> Result<String, LlmError> {
    let body = request_body(&request, request.max_output_tokens);
    generate_response(http, profile, api_key, request, body, None, true).await
}

pub(super) async fn generate_with_image(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    image: &[u8],
) -> Result<String, LlmError> {
    let body = image_request_body(&request, request.max_output_tokens, image);
    generate_response(
        http,
        profile,
        api_key,
        request,
        body,
        Some(Duration::from_millis(profile.timeout_ms)),
        false,
    )
    .await
}

async fn generate_response(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    mut body: Value,
    timeout: Option<Duration>,
    allow_budget_retry: bool,
) -> Result<String, LlmError> {
    let mut max_output_tokens = request.max_output_tokens;
    for attempt in 0..=1 {
        body["max_output_tokens"] = json!(max_output_tokens);
        super::models_dev::apply_request(&mut body, profile, &request);
        let explicit_reasoning = super::reasoning::apply_explicit_control(
            &mut body,
            &profile.provider,
            &request,
            super::models_dev::model_details(profile, request.model).as_ref(),
        )?;
        let response = generation_request(http, api_key, &body, timeout)
            .send()
            .await
            .map_err(network_error)?;
        match parse_http_response(response).await? {
            ParsedResponse::Complete(text) => return Ok(text),
            ParsedResponse::Incomplete(reason) => {
                if attempt == 0 {
                    if let Some(retry_limit) = retry_output_token_limit(
                        &reason,
                        max_output_tokens,
                        allow_budget_retry && !explicit_reasoning,
                    ) {
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

fn generation_request(
    http: &reqwest::Client,
    api_key: &str,
    body: &Value,
    timeout: Option<Duration>,
) -> reqwest::RequestBuilder {
    let request = http
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(api_key)
        .json(body);
    if let Some(timeout) = timeout {
        request.timeout(timeout)
    } else {
        request
    }
}

fn image_request_body(request: &LlmRequest<'_>, max_output_tokens: u32, image: &[u8]) -> Value {
    let mut body = request_body(request, max_output_tokens);
    body["input"] = json!([{ "role": "user", "content": [
        { "type": "input_text", "text": request.input },
        { "type": "input_image", "image_url": format!("data:image/png;base64,{}", STANDARD.encode(image)) }
    ] }]);
    body
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

fn retry_output_token_limit(reason: &str, current: u32, allow_budget_retry: bool) -> Option<u32> {
    if !allow_budget_retry || reason != "max_output_tokens" {
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
        retry_after: None,
    }
}

async fn parse_http_response(response: reqwest::Response) -> Result<ParsedResponse, LlmError> {
    let status = response.status();
    if !status.is_success() {
        return Err(response_status_error(response, status).await);
    }
    let value: Value = response.json().await.map_err(invalid_response)?;
    parse_response(&value)
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
            retry_after: None,
        })
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
    parse_catalog(
        http.get("https://api.openai.com/v1/models")
            .timeout(Duration::from_millis(profile.timeout_ms))
            .bearer_auth(api_key),
        &profile.provider,
    )
    .await
}

fn extract_text(value: &Value) -> Option<String> {
    let text = value
        .get("output")?
        .as_array()?
        .iter()
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<String>();
    Some(text.trim().to_owned()).filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_native_image_request_with_profile_timeout() {
        let profile = ApiProfile {
            timeout_ms: 12_345,
            ..ApiProfile::default()
        };
        let request = LlmRequest {
            model: "gpt-5.4-mini",
            instructions: "Describe the image",
            input: "What is visible?",
            max_output_tokens: 1_024,
            thinking_enabled: false,
            reasoning_control: None,
        };
        let body = image_request_body(&request, request.max_output_tokens, b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            body,
            json!({
                "model": "gpt-5.4-mini",
                "instructions": "Describe the image",
                "input": [{ "role": "user", "content": [
                    { "type": "input_text", "text": "What is visible?" },
                    { "type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo=" }
                ] }],
                "max_output_tokens": 1_024,
                "reasoning": { "effort": "none" }
            })
        );
        let http = crate::http_client();
        let built = generation_request(
            &http,
            "test-key",
            &body,
            Some(Duration::from_millis(profile.timeout_ms)),
        )
        .build()
        .unwrap();
        assert_eq!(built.method(), reqwest::Method::POST);
        assert_eq!(built.url().as_str(), "https://api.openai.com/v1/responses");
        assert_eq!(built.headers()["authorization"], "Bearer test-key");
        assert_eq!(built.timeout(), Some(&Duration::from_millis(12_345)));
        let sent: Value =
            serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(sent, body);
        assert!(sent.get("stream").is_none());

        let text_body = request_body(&request, request.max_output_tokens);
        let text_request = generation_request(&http, "test-key", &text_body, None)
            .build()
            .unwrap();
        assert_eq!(text_body["input"], "What is visible?");
        assert!(text_request.timeout().is_none());
    }

    #[test]
    fn image_body_preserves_reasoning_and_hard_output_budget() {
        for (model, thinking_enabled) in [("gpt-5-mini", true), ("gpt-4o-mini", false)] {
            let request = LlmRequest {
                model,
                instructions: "Describe the image",
                input: "What is visible?",
                max_output_tokens: 1_024,
                thinking_enabled,
                reasoning_control: None,
            };
            let body = image_request_body(&request, 1_024, b"png");
            assert_eq!(body["max_output_tokens"], 1_024);
            assert_eq!(
                retry_output_token_limit("max_output_tokens", 1_024, false),
                None
            );
            let mut text_body = request_body(&request, 1_024);
            text_body["input"] = body["input"].clone();
            assert_eq!(body, text_body);
        }
    }

    #[test]
    fn parses_image_answers_and_rejects_responses_without_text() {
        let response = json!({
            "status": "completed",
            "output": [
                { "type": "reasoning", "summary": [] },
                { "type": "message", "content": [
                    { "type": "output_text", "text": " A blue sign. " }
                ] }
            ]
        });
        let ParsedResponse::Complete(text) = parse_response(&response).unwrap() else {
            panic!("expected a complete response");
        };
        assert_eq!(text, "A blue sign.");
        for response in [
            json!({"status": "completed", "output": []}),
            json!({"status": "completed", "output_text": "  "}),
        ] {
            let error = match parse_response(&response) {
                Err(error) => error,
                Ok(_) => panic!("expected a response without text to fail"),
            };
            assert_eq!(error.code, "llm.invalid_response");
            assert!(!error.retryable);
        }
    }

    #[test]
    fn concatenates_all_output_text_parts_in_order_without_other_text_fields() {
        let value = json!({
            "output_text": "Do not use this aggregate field",
            "text": "Do not use this text field",
            "output": [
                {"type": "reasoning", "summary": [{"text": "private reasoning"}]},
                {"type": "message", "content": [
                    {"type": "refusal", "text": "Not body text"},
                    {"text": "Untyped text"},
                    {"type": "output_text", "text": " A"},
                    {"type": "output_text", "text": " blue"}
                ]},
                {"type": "message", "content": [
                    {"type": "output_text", "text": " sign. "}
                ]}
            ]
        });
        assert_eq!(extract_text(&value).as_deref(), Some("A blue sign."));
        assert_eq!(
            extract_text(&json!({"output_text": "aggregate only"})),
            None
        );
        assert_eq!(
            extract_text(&json!({"output": [{"content": [
                {"type": "refusal", "text": "not an answer"},
                {"type": "output_text", "text": "  "}
            ]}]})),
            None
        );
    }

    async fn http_error(response: reqwest::Response) -> LlmError {
        match parse_http_response(response).await {
            Err(error) => error,
            Ok(_) => panic!("expected HTTP response parsing to fail"),
        }
    }

    #[tokio::test]
    async fn checks_http_status_before_parsing_json() {
        for (status, code, retryable) in [
            (503, "llm.provider_unavailable", true),
            (429, "llm.rate_limited", true),
            (403, "llm.permission_denied", false),
        ] {
            let response = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(status)
                .body("<html>upstream error</html>")
                .unwrap()
                .into();
            let error = http_error(response).await;
            assert_eq!(error.code, code);
            assert_eq!(error.retryable, retryable);
            assert!(error.detail.contains(&status.to_string()));
        }
        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(401)
            .body(r#"{"error":{"message":"Invalid key"}}"#)
            .unwrap()
            .into();
        let error = http_error(response).await;
        assert_eq!(error.code, "llm.authentication_failed");
        assert_eq!(error.detail, "Invalid key");
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn distinguishes_body_transport_errors_from_invalid_json() {
        let body = reqwest::Body::wrap_stream(futures_util::stream::iter([Err::<Vec<u8>, _>(
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "body interrupted"),
        )]));
        let response: reqwest::Response = tokio_tungstenite::tungstenite::http::Response::builder()
            .body(body)
            .unwrap()
            .into();
        // This in-memory stream exercises the Decode -> Body -> IO error chain.
        let raw_error = response.json::<Value>().await.unwrap_err();
        assert!(raw_error.is_decode());
        assert!(!raw_error.is_body());
        let mut source = std::error::Error::source(&raw_error);
        let mut saw_io_error = false;
        while let Some(cause) = source {
            if let Some(error) = cause.downcast_ref::<std::io::Error>() {
                assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
                saw_io_error = true;
            }
            source = cause.source();
        }
        assert!(saw_io_error);
        let error = invalid_response(raw_error);
        assert_eq!(error.code, "llm.network_failed");
        assert!(error.retryable);

        for body in [
            "not JSON",
            r#"{"output":"#,
            r#"{"output": [}"#,
            r#"{"output": [],}"#,
        ] {
            let response: reqwest::Response =
                tokio_tungstenite::tungstenite::http::Response::builder()
                    .body(body)
                    .unwrap()
                    .into();
            let raw_error = response.json::<Value>().await.unwrap_err();
            assert!(raw_error.is_decode());
            let json_error = std::error::Error::source(&raw_error)
                .unwrap()
                .downcast_ref::<serde_json::Error>()
                .unwrap();
            assert!(!json_error.is_io());
            assert!(json_error.is_syntax() || json_error.is_eof());
            let error = invalid_response(raw_error);
            assert_eq!(error.code, "llm.invalid_response");
            assert!(!error.retryable);
        }
    }

    #[tokio::test]
    async fn json_body_timeout_is_a_retryable_network_error() {
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(|| async {
                axum::body::Body::from_stream(futures_util::stream::once(async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok::<_, std::io::Error>("{}")
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let response = crate::http_client_builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(endpoint)
            .timeout(Duration::from_millis(500))
            .send()
            .await
            .unwrap();
        let error = http_error(response).await;
        server.abort();
        assert_eq!(error.code, "llm.timeout");
        assert!(error.retryable);
    }

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
            reasoning_control: None,
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
            retry_output_token_limit("max_output_tokens", 1_024, true),
            Some(2_048)
        );
        assert_eq!(
            retry_output_token_limit("max_output_tokens", 10_000, true),
            Some(16_384)
        );
        assert_eq!(
            retry_output_token_limit("max_output_tokens", 16_384, true),
            None
        );
        assert_eq!(
            retry_output_token_limit("content_filter", 1_024, true),
            None
        );
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
