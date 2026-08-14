use futures_util::StreamExt;
use reqwest::StatusCode;
use serde_json::{json, Value};

use super::{LlmError, LlmProgress, LlmRequest};

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub(super) async fn generate(
    http: &reqwest::Client,
    api_key: &str,
    request: LlmRequest<'_>,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    let model = model_id(request.model)?;
    let method = if on_progress.is_some() {
        "streamGenerateContent?alt=sse"
    } else {
        "generateContent"
    };
    let response = http
        .post(format!("{BASE_URL}/models/{model}:{method}"))
        .header("x-goog-api-key", api_key)
        .json(&request_body(&request))
        .send()
        .await
        .map_err(network_error)?;
    let status = response.status();
    if !status.is_success() {
        let value = response.json().await.map_err(invalid_response)?;
        return Err(status_error(status, &value));
    }
    if let Some(progress) = on_progress {
        return stream_response(response, progress).await;
    }
    let value = response.json().await.map_err(invalid_response)?;
    extract_text(&value).ok_or_else(|| LlmError {
        code: "llm.invalid_response",
        detail: "Gemini response did not contain text".into(),
        retryable: false,
    })
}

pub(super) async fn list_models(
    http: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<String>, LlmError> {
    let response = http
        .get(format!("{BASE_URL}/models"))
        .header("x-goog-api-key", api_key)
        .send()
        .await
        .map_err(network_error)?;
    let status = response.status();
    let value = response.json().await.map_err(invalid_response)?;
    if !status.is_success() {
        return Err(status_error(status, &value));
    }
    let models = extract_models(&value);
    if models.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: "Gemini did not return any models that support generateContent".into(),
            retryable: false,
        });
    }
    Ok(models)
}

fn request_body(request: &LlmRequest<'_>) -> Value {
    json!({
        "systemInstruction": { "parts": [{ "text": request.instructions }] },
        "contents": [{ "role": "user", "parts": [{ "text": request.input }] }],
        "generationConfig": { "maxOutputTokens": request.max_output_tokens }
    })
}

fn model_id(model: &str) -> Result<&str, LlmError> {
    let model = model.trim().strip_prefix("models/").unwrap_or(model.trim());
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
    while let Some(chunk) = chunks.next().await {
        buffer.extend_from_slice(&chunk.map_err(network_error)?);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line = buffer.drain(..=newline).collect::<Vec<_>>();
            process_sse_line(&line, &mut output, on_progress)?;
        }
    }
    if !buffer.is_empty() {
        process_sse_line(&buffer, &mut output, on_progress)?;
    }
    let output = output.trim().to_owned();
    if output.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: "Gemini stream did not contain text".into(),
            retryable: false,
        });
    }
    Ok(output)
}

fn process_sse_line(
    line: &[u8],
    output: &mut String,
    on_progress: &LlmProgress,
) -> Result<(), LlmError> {
    let line = std::str::from_utf8(line).map_err(|error| LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
    })?;
    let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else {
        return Ok(());
    };
    let value = serde_json::from_str(data).map_err(|error| LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
    })?;
    if let Some(text) = extract_raw_text(&value) {
        output.push_str(&text);
        on_progress(output);
    }
    Ok(())
}

fn extract_text(value: &Value) -> Option<String> {
    let text = extract_raw_text(value)?.trim().to_owned();
    Some(text).filter(|text| !text.is_empty())
}

fn extract_raw_text(value: &Value) -> Option<String> {
    let text = value
        .get("candidates")?
        .as_array()?
        .iter()
        .flat_map(|candidate| {
            candidate
                .pointer("/content/parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
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
        .map(|name| name.strip_prefix("models/").unwrap_or(name).trim())
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_request_to_native_gemini_shape() {
        let body = request_body(&LlmRequest {
            model: "gemini-2.5-flash",
            instructions: "Translate",
            input: "こんにちは",
            max_output_tokens: 256,
            thinking_enabled: false,
        });
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Translate");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "こんにちは");
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 256);
    }

    #[test]
    fn extracts_text_and_generate_models() {
        let response = json!({
            "candidates": [{ "content": { "parts": [{ "text": "hello" }] } }]
        });
        assert_eq!(extract_text(&response).as_deref(), Some("hello"));

        let models = extract_models(&json!({ "models": [
            { "name": "models/gemini-2.5-flash", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/embedding-001", "supportedGenerationMethods": ["embedContent"] }
        ] }));
        assert_eq!(models, ["gemini-2.5-flash"]);
    }

    #[test]
    fn appends_stream_chunks() {
        let mut output = String::new();
        let snapshots = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = std::sync::Arc::clone(&snapshots);
        let progress = move |text: &str| captured.lock().unwrap().push(text.to_owned());
        process_sse_line(
            br#"data: {"candidates":[{"content":{"parts":[{"text":"hel"}]}}]}"#,
            &mut output,
            &progress,
        )
        .unwrap();
        process_sse_line(
            br#"data: {"candidates":[{"content":{"parts":[{"text":" lo"}]}}]}"#,
            &mut output,
            &progress,
        )
        .unwrap();
        assert_eq!(output, "hel lo");
        assert_eq!(*snapshots.lock().unwrap(), ["hel", "hel lo"]);
    }
}
