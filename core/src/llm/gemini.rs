use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::time::Duration;

use crate::config::ApiProfile;

use super::{LlmError, LlmProgress, LlmRequest};

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub(super) async fn generate(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    let mut body = request_body(&request);
    super::models_dev::apply_request(&mut body, profile, &request);
    super::reasoning::apply_explicit_control(
        &mut body,
        &profile.provider,
        &request,
        super::models_dev::model_details(profile, request.model).as_ref(),
    )?;
    let request = generation_request(
        http,
        profile,
        api_key,
        request.model,
        &body,
        on_progress.is_some(),
    )?;
    generate_response(request, on_progress).await
}

pub(super) async fn generate_with_image(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    image: &[u8],
) -> Result<String, LlmError> {
    let mut body = image_request_body(&request, image);
    super::models_dev::apply_request(&mut body, profile, &request);
    super::reasoning::apply_explicit_control(
        &mut body,
        &profile.provider,
        &request,
        super::models_dev::model_details(profile, request.model).as_ref(),
    )?;
    let request = generation_request(http, profile, api_key, request.model, &body, false)?;
    generate_response(request, None).await
}

fn generation_request(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    model: &str,
    body: &Value,
    streaming: bool,
) -> Result<reqwest::RequestBuilder, LlmError> {
    let model = model_id(model)?;
    if body["contents"].as_array().is_some_and(|contents| {
        contents.iter().any(|content| {
            content["parts"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part.get("inlineData").is_some()))
        })
    }) {
        super::http::max_serialized_body(body, 20_000_000, "Gemini inline request (20 MB)")?;
    }
    let method = if streaming {
        "streamGenerateContent?alt=sse"
    } else {
        "generateContent"
    };
    Ok(http
        .post(format!("{BASE_URL}/models/{model}:{method}"))
        .timeout(Duration::from_millis(profile.timeout_ms))
        .header("x-goog-api-key", api_key)
        .json(body))
}

async fn generate_response(
    request: reqwest::RequestBuilder,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    let response = request.send().await.map_err(network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(response_error(response).await);
    }
    if let Some(progress) = on_progress {
        return stream_response(response, progress).await;
    }
    let value = response.json().await.map_err(invalid_response)?;
    parse_response(&value)
}

pub(super) async fn list_models(
    http: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<String>, LlmError> {
    list_model_catalog(http, api_key)
        .await
        .map(|catalog| catalog.models)
}

pub(super) async fn list_model_catalog(
    http: &reqwest::Client,
    api_key: &str,
) -> Result<super::model_catalog::ModelCatalog, LlmError> {
    let response = http
        .get(format!("{BASE_URL}/models"))
        .header("x-goog-api-key", api_key)
        .send()
        .await
        .map_err(network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(response_error(response).await);
    }
    let value: Value = response.json().await.map_err(invalid_response)?;
    let models = extract_models(&value);
    if models.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: "Gemini did not return any models that support generateContent".into(),
            retryable: false,
            retry_after: None,
        });
    }
    let model_details = value
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let model = entry.get("name")?.as_str()?.strip_prefix("models/")?;
            if !models.iter().any(|id| id == model) {
                return None;
            }
            let details = super::model_catalog::ModelDetails::from_native(entry, "gemini");
            Some((model.to_owned(), details))
        })
        .collect();
    Ok(super::model_catalog::ModelCatalog {
        models,
        model_details,
    })
}

fn image_request_body(request: &LlmRequest<'_>, image: &[u8]) -> Value {
    let mut body = request_body(request);
    // Vision uses a hard total generation limit, including thinking tokens.
    body["generationConfig"]["maxOutputTokens"] = json!(request.max_output_tokens);
    body["contents"][0]["parts"] = json!([
        { "text": request.input },
        { "inlineData": { "mimeType": "image/png", "data": STANDARD.encode(image) } }
    ]);
    body
}

fn request_body(request: &LlmRequest<'_>) -> Value {
    let mut body = json!({
        "systemInstruction": { "parts": [{ "text": request.instructions }] },
        "contents": [{ "role": "user", "parts": [{ "text": request.input }] }],
        "generationConfig": { "maxOutputTokens": request.max_output_tokens }
    });
    let model = request.model.trim().trim_start_matches("models/");
    let (thinking, reserve) = if model.starts_with("gemini-2.5-flash") {
        let budget = if request.thinking_enabled { 1024 } else { 0 };
        (Some(json!({ "thinkingBudget": budget })), budget)
    } else if model.starts_with("gemini-2.5-pro") {
        let budget = if request.thinking_enabled { 2048 } else { 128 };
        (Some(json!({ "thinkingBudget": budget })), budget)
    } else if (model.starts_with("gemini-3.") || model.starts_with("gemini-3-"))
        && !model.contains("-image")
    {
        (
            Some(json!({ "thinkingLevel": if request.thinking_enabled { "high" } else { "low" } })),
            if request.thinking_enabled { 8192 } else { 4096 },
        )
    } else {
        (None, 0)
    };
    if let Some(thinking) = thinking {
        body["generationConfig"]["thinkingConfig"] = thinking;
    }
    // Thinking and visible text share the output limit. Levels are not hard budgets.
    body["generationConfig"]["maxOutputTokens"] =
        json!(request.max_output_tokens.max(512).saturating_add(reserve));
    body
}

fn model_id(model: &str) -> Result<&str, LlmError> {
    let model = model.trim().trim_prefix("models/");
    if model.is_empty()
        || model.len() > 200
        || !model
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_' | b'.'))
    {
        return Err(LlmError {
            code: "llm.invalid_profile",
            detail: "Gemini model ID is invalid".into(),
            retryable: false,
            retry_after: None,
        });
    }
    Ok(model)
}

async fn stream_response(
    response: reqwest::Response,
    on_progress: &LlmProgress,
) -> Result<String, LlmError> {
    let mut chunks = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut output = String::new();
    let mut event_data = String::new();
    let mut completed = false;
    while let Some(chunk) = chunks.next().await {
        buffer.extend_from_slice(&chunk.map_err(network_error)?);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line = buffer.drain(..=newline).collect::<Vec<_>>();
            completed |= process_sse_line(&line, &mut event_data, &mut output, on_progress)?;
        }
    }
    if !buffer.is_empty() {
        completed |= process_sse_line(&buffer, &mut event_data, &mut output, on_progress)?;
    }
    completed |= process_sse_line(b"\n", &mut event_data, &mut output, on_progress)?;
    completed_response(output, completed)
}

fn process_sse_line(
    line: &[u8],
    event_data: &mut String,
    output: &mut String,
    on_progress: &LlmProgress,
) -> Result<bool, LlmError> {
    let line = std::str::from_utf8(line).map_err(|error| LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
        retry_after: None,
    })?;
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some(data) = line.strip_prefix("data:") {
        event_data.push_str(data.trim_prefix(' '));
        event_data.push('\n');
    }
    if !line.is_empty() || event_data.is_empty() {
        return Ok(false);
    }
    let value = serde_json::from_str(event_data).map_err(|error| LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
        retry_after: None,
    })?;
    event_data.clear();
    let completed = response_completed(&value)?;
    if let Some(text) = extract_raw_text(&value) {
        output.push_str(&text);
        on_progress(output);
    }
    Ok(completed)
}

fn response_completed(value: &Value) -> Result<bool, LlmError> {
    if let Some(error) = value.get("error") {
        let status = error
            .get("code")
            .and_then(Value::as_u64)
            .and_then(|code| u16::try_from(code).ok())
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return Err(status_error(status, value));
    }
    if let Some(reason) = value
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
    {
        return Err(response_failure(
            format!("Gemini blocked the prompt: {reason}"),
            false,
        ));
    }
    match first_candidate(value)
        .and_then(|candidate| candidate.get("finishReason"))
        .and_then(Value::as_str)
    {
        Some("STOP") => Ok(true),
        None | Some("" | "FINISH_REASON_UNSPECIFIED") => Ok(false),
        Some(reason) => Err(response_failure(
            format!("Gemini generation stopped: {reason}"),
            false,
        )),
    }
}

fn completed_response(output: String, completed: bool) -> Result<String, LlmError> {
    if !completed {
        return Err(response_failure(
            "Gemini response ended without a completion signal".into(),
            true,
        ));
    }
    let output = output.trim().to_owned();
    if output.is_empty() {
        return Err(response_failure(
            "Gemini completed the response without text".into(),
            true,
        ));
    }
    Ok(output)
}

fn response_failure(detail: String, retryable: bool) -> LlmError {
    LlmError {
        code: "llm.request_failed",
        detail,
        retryable,
        retry_after: None,
    }
}

fn first_candidate(value: &Value) -> Option<&Value> {
    value
        .get("candidates")?
        .as_array()?
        .iter()
        .find(|candidate| candidate.get("index").and_then(Value::as_u64).unwrap_or(0) == 0)
}

fn parse_response(value: &Value) -> Result<String, LlmError> {
    completed_response(
        extract_raw_text(value).unwrap_or_default(),
        response_completed(value)?,
    )
}

fn extract_raw_text(value: &Value) -> Option<String> {
    let text = first_candidate(value)?
        .pointer("/content/parts")?
        .as_array()?
        .iter()
        .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    Some(text).filter(|text| !text.is_empty())
}

fn extract_models(value: &Value) -> Vec<String> {
    let mut models = value
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|model| {
            model
                .get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .is_some_and(|methods| methods.iter().any(|method| method == "generateContent"))
        })
        .filter_map(|model| model.get("name").and_then(Value::as_str))
        .map(|name| name.trim_prefix("models/").trim())
        .filter(|name| !name.is_empty() && name.len() <= 200)
        .map(str::to_owned)
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
        retry_after: None,
    }
}

fn invalid_response(error: reqwest::Error) -> LlmError {
    if error.is_timeout() || error.is_body() {
        return network_error(error);
    }
    LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
        retry_after: None,
    }
}

async fn response_error(response: reqwest::Response) -> LlmError {
    let status = response.status();
    let value = response.json().await.unwrap_or_else(|_| json!({
        "error": { "message": format!("Gemini returned HTTP {status} without a JSON error body") }
    }));
    status_error(status, &value)
}

fn status_error(status: StatusCode, value: &Value) -> LlmError {
    let detail = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("Gemini request failed")
        .to_owned();
    let (code, retryable) = match status.as_u16() {
        400 => ("llm.invalid_request", false),
        401 | 403 => ("llm.authentication_failed", false),
        404 => ("llm.model_not_found", false),
        408 => ("llm.timeout", true),
        429 => ("llm.rate_limited", true),
        500..=599 => ("llm.provider_unavailable", true),
        _ => ("llm.request_failed", false),
    };
    LlmError {
        code,
        detail,
        retryable,
        retry_after: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_native_image_request_without_streaming() {
        let profile = ApiProfile {
            timeout_ms: 12_345,
            ..ApiProfile::default()
        };
        let request = LlmRequest {
            model: "models/gemini-2.5-flash",
            instructions: "Describe the image",
            input: "What is visible?",
            max_output_tokens: 256,
            thinking_enabled: false,
            reasoning_control: None,
        };
        let body = image_request_body(&request, b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            body,
            json!({
                "systemInstruction": { "parts": [{ "text": "Describe the image" }] },
                "contents": [{ "role": "user", "parts": [
                    { "text": "What is visible?" },
                    { "inlineData": { "mimeType": "image/png", "data": "iVBORw0KGgo=" } }
                ] }],
                "generationConfig": {
                    "maxOutputTokens": 256,
                    "thinkingConfig": { "thinkingBudget": 0 }
                }
            })
        );
        let http = reqwest::Client::new();
        let built = generation_request(&http, &profile, "test-key", request.model, &body, false)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(built.method(), reqwest::Method::POST);
        assert_eq!(
            built.url().as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
        );
        assert_eq!(built.headers()["x-goog-api-key"], "test-key");
        assert_eq!(built.timeout(), Some(&Duration::from_millis(12_345)));
        let sent: Value =
            serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(sent, body);
        assert!(built.url().query().is_none());
        assert!(sent.get("stream").is_none());

        let text_body = request_body(&request);
        assert_eq!(
            text_body["contents"][0]["parts"],
            json!([{ "text": "What is visible?" }])
        );
        let stream_request =
            generation_request(&http, &profile, "test-key", request.model, &text_body, true)
                .unwrap()
                .build()
                .unwrap();
        assert!(stream_request
            .url()
            .path()
            .ends_with(":streamGenerateContent"));
        assert_eq!(stream_request.url().query(), Some("alt=sse"));
    }

    #[test]
    fn image_body_preserves_model_thinking_and_output_budgets() {
        for (model, thinking_enabled) in [
            ("gemini-2.5-flash", false),
            ("gemini-2.5-flash", true),
            ("gemini-2.5-pro", false),
            ("gemini-2.5-pro", true),
            ("gemini-3.7-flash", false),
            ("gemini-3.7-flash", true),
        ] {
            let request = LlmRequest {
                model,
                instructions: "Describe the image",
                input: "What is visible?",
                max_output_tokens: 256,
                thinking_enabled,
                reasoning_control: None,
            };
            let body = image_request_body(&request, b"png");
            let mut expected = request_body(&request);
            assert!(
                expected["generationConfig"]["maxOutputTokens"]
                    .as_u64()
                    .unwrap()
                    >= 512
            );
            expected["generationConfig"]["maxOutputTokens"] = json!(256);
            expected["contents"][0]["parts"] = body["contents"][0]["parts"].clone();
            assert_eq!(body, expected);
        }
    }

    #[test]
    fn inline_limit_counts_the_complete_encoded_json_body() {
        let profile = ApiProfile::default();
        let http = reqwest::Client::new();
        let request = LlmRequest {
            model: "gemini-2.5-flash",
            instructions: "Describe",
            input: "",
            max_output_tokens: 256,
            thinking_enabled: false,
            reasoning_control: None,
        };
        let mut body = image_request_body(&request, b"png");
        let overhead = serde_json::to_vec(&body).unwrap().len();
        body["contents"][0]["parts"][0]["text"] = json!("a".repeat(20_000_000 - overhead));
        let _ = generation_request(&http, &profile, "key", request.model, &body, false).unwrap();
        // A newline takes two bytes in JSON, even though it is one input byte.
        body["systemInstruction"]["parts"][0]["text"] = json!("Describe\n");
        let error =
            generation_request(&http, &profile, "key", request.model, &body, false).unwrap_err();
        assert!(error.detail.contains("20 MB"));
        assert!(error.detail.contains("20000002 bytes > 20000000 bytes"));
        assert!(!error.retryable);
        // The same large prompt has no inline-protocol limit when sent as text.
        body["contents"][0]["parts"].as_array_mut().unwrap().pop();
        let _ = generation_request(&http, &profile, "key", request.model, &body, false).unwrap();
        drop(body);

        let encoded = image_request_body(&request, &vec![0; 15_000_000]);
        let error =
            generation_request(&http, &profile, "key", request.model, &encoded, false).unwrap_err();
        assert!(error.detail.contains("serialized JSON body limit"));
        assert!(!error.retryable);
    }

    #[test]
    fn parses_image_answers_with_completion_and_text_only_parts() {
        let response = json!({"candidates": [{
            "finishReason": "STOP",
            "content": {"parts": [
                { "thought": true, "text": "private reasoning" },
                { "inlineData": { "mimeType": "image/png", "data": "cG5n" } },
                { "text": " A blue" },
                { "text": " sign. " }
            ]}
        }]});
        assert_eq!(parse_response(&response).unwrap(), "A blue sign.");
        let mut unfinished = response.clone();
        unfinished["candidates"][0]
            .as_object_mut()
            .unwrap()
            .remove("finishReason");
        let error = parse_response(&unfinished).unwrap_err();
        assert!(error.detail.contains("without a completion signal"));
        assert!(error.retryable);
        let blocked = json!({
            "promptFeedback": { "blockReason": "SAFETY" },
            "candidates": response["candidates"]
        });
        let error = parse_response(&blocked).unwrap_err();
        assert!(error.detail.contains("SAFETY"));
        assert!(!error.retryable);
    }

    fn sse_response(chunks: Vec<Vec<u8>>) -> reqwest::Response {
        let body = reqwest::Body::wrap_stream(futures_util::stream::iter(
            chunks.into_iter().map(Ok::<_, std::io::Error>),
        ));
        tokio_tungstenite::tungstenite::http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(body)
            .unwrap()
            .into()
    }

    #[tokio::test]
    async fn rejects_unsuccessful_or_unfinished_gemini_streams() {
        for (event, reason, retryable) in [
            (
                json!({"candidates":[{"finishReason":"MAX_TOKENS"}]}),
                "MAX_TOKENS",
                false,
            ),
            (
                json!({"promptFeedback":{"blockReason":"SAFETY"}}),
                "SAFETY",
                false,
            ),
            (
                json!({"error":{"code":503,"message":"Temporarily unavailable"}}),
                "Temporarily unavailable",
                true,
            ),
            (
                json!({"usageMetadata":{"totalTokenCount":12}}),
                "completion",
                true,
            ),
        ] {
            let partial =
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"partial\"}]}}]}\n\n";
            let response = sse_response(vec![format!("{partial}data: {event}\n\n").into_bytes()]);
            let error = stream_response(response, &|_| {}).await.unwrap_err();
            assert!(error.detail.contains(reason), "{error:?}");
            assert_eq!(error.retryable, retryable);
        }
    }

    #[tokio::test]
    async fn accepts_complete_gemini_sse_events_across_byte_boundaries() {
        let stream = concat!(
            ": heartbeat\r\n\r\n",
            "data: {\"candidates\":[{\"content\":\n",
            "data: {\"parts\":[{\"text\":\"你好\"}]}}]}\n\n",
            "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n\n",
            "data: {\"usageMetadata\":{\"totalTokenCount\":12}}\n\n",
        );
        let chunks = stream.as_bytes().iter().map(|byte| vec![*byte]).collect();
        assert_eq!(
            stream_response(sse_response(chunks), &|_| {})
                .await
                .unwrap(),
            "你好"
        );
    }

    #[test]
    fn gemini_thinking_and_output_budgets_follow_model_capabilities() {
        for (model, thinking, expected_config, reserve) in [
            ("gemini-2.5-flash", false, json!({"thinkingBudget":0}), 0),
            (
                "gemini-2.5-flash",
                true,
                json!({"thinkingBudget":1024}),
                1024,
            ),
            ("gemini-2.5-pro", false, json!({"thinkingBudget":128}), 128),
            (
                "models/gemini-3.7-flash",
                false,
                json!({"thinkingLevel":"low"}),
                4096,
            ),
        ] {
            let body = request_body(&LlmRequest {
                model,
                instructions: "Translate",
                input: "Hello",
                max_output_tokens: 256,
                thinking_enabled: thinking,
                reasoning_control: None,
            });
            assert_eq!(body["generationConfig"]["thinkingConfig"], expected_config);
            assert_eq!(
                body["generationConfig"]["maxOutputTokens"],
                json!(512 + reserve)
            );
        }
    }

    #[test]
    fn maps_request_to_native_gemini_shape() {
        let body = request_body(&LlmRequest {
            model: "gemini-2.5-flash",
            instructions: "Translate",
            input: "こんにちは",
            max_output_tokens: 256,
            thinking_enabled: false,
            reasoning_control: None,
        });
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Translate");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "こんにちは");
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 512);
    }

    #[test]
    fn extracts_text_and_generate_models() {
        let response = json!({
            "candidates": [{ "finishReason": "STOP", "content": { "parts": [
                { "text": "private reasoning", "thought": true },
                { "text": "hello" }
            ] } }]
        });
        assert_eq!(parse_response(&response).unwrap(), "hello");

        let models = extract_models(&json!({ "models": [
            { "name": "models/gemini-2.5-flash", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/embedding-001", "supportedGenerationMethods": ["embedContent"] }
        ] }));
        assert_eq!(models, ["gemini-2.5-flash"]);
    }

    #[test]
    fn json_responses_preserve_failure_reasons_and_do_not_merge_candidates() {
        for reason in ["MAX_TOKENS", "SAFETY", "RECITATION"] {
            let response = json!({"candidates":[{"finishReason":reason,"content":{"parts":[{"text":"partial"}]}}]});
            let error = parse_response(&response).unwrap_err();
            assert_eq!(error.code, "llm.request_failed");
            assert!(error.detail.contains(reason));
            assert!(!error.retryable);
        }
        let empty = parse_response(&json!({"candidates":[{"finishReason":"STOP"}]})).unwrap_err();
        assert!(empty.detail.contains("without text"));
        assert!(empty.retryable);
        let response = json!({"candidates":[
            {"index":1,"finishReason":"MAX_TOKENS","content":{"parts":[{"text":"alternative"}]}},
            {"index":0,"finishReason":"STOP","content":{"parts":[{"text":" translation "}]}}
        ]});
        assert_eq!(parse_response(&response).unwrap(), "translation");
    }

    #[tokio::test]
    async fn non_json_http_errors_preserve_status_and_retryability() {
        for (status, code, retryable) in [
            (503, "llm.provider_unavailable", true),
            (429, "llm.rate_limited", true),
            (403, "llm.authentication_failed", false),
        ] {
            let response = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(status)
                .body("<html>upstream error</html>")
                .unwrap()
                .into();
            let error = response_error(response).await;
            assert_eq!(error.code, code);
            assert_eq!(error.retryable, retryable);
            assert!(error.detail.contains(&status.to_string()));
        }
    }

    #[test]
    fn appends_stream_chunks() {
        let mut output = String::new();
        let mut event_data = String::new();
        let snapshots = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = std::sync::Arc::clone(&snapshots);
        let progress = move |text: &str| captured.lock().unwrap().push(text.to_owned());
        process_sse_line(
            br#"data: {"candidates":[{"content":{"parts":[{"text":"hel"}]}}]}"#,
            &mut event_data,
            &mut output,
            &progress,
        )
        .unwrap();
        process_sse_line(b"\n", &mut event_data, &mut output, &progress).unwrap();
        process_sse_line(
            br#"data: {"candidates":[{"content":{"parts":[{"text":" lo"}]}}]}"#,
            &mut event_data,
            &mut output,
            &progress,
        )
        .unwrap();
        process_sse_line(b"\n", &mut event_data, &mut output, &progress).unwrap();
        assert_eq!(output, "hel lo");
        assert_eq!(*snapshots.lock().unwrap(), ["hel", "hel lo"]);
    }
}
