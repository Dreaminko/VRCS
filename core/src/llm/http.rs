use reqwest::StatusCode;
use serde_json::{json, Value};

use super::LlmError;

pub(super) async fn list_models(request: reqwest::RequestBuilder) -> Result<Vec<String>, LlmError> {
    list_model_catalog(request, "")
        .await
        .map(|catalog| catalog.models)
}

pub(super) async fn list_model_catalog(
    request: reqwest::RequestBuilder,
    provider: &str,
) -> Result<super::model_catalog::ModelCatalog, LlmError> {
    let response = request.send().await.map_err(network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(response_status_error(response, status).await);
    }
    let value: Value = response.json().await.map_err(invalid_response)?;
    let models = extract_model_ids(&value);
    if models.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: "The LLM service did not return any models".into(),
            retryable: false,
            retry_after: None,
        });
    }
    let model_details = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.trim();
            if !models.iter().any(|model| model == id) {
                return None;
            }
            let details = super::model_catalog::ModelDetails::from_native(entry, provider);
            (!details.field_sources.is_empty()).then(|| (id.to_owned(), details))
        })
        .collect();
    Ok(super::model_catalog::ModelCatalog {
        models,
        model_details,
    })
}

pub(super) fn network_error(error: reqwest::Error) -> LlmError {
    LlmError {
        code: network_error_code(&error),
        detail: error.to_string(),
        retryable: true,
        retry_after: None,
    }
}

pub(super) fn invalid_response(error: reqwest::Error) -> LlmError {
    if error.is_timeout() || error.is_body() {
        return network_error(error);
    }
    // Response::bytes() also wraps body collection failures as Decode errors.
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        if cause.is::<std::io::Error>()
            || cause
                .downcast_ref::<serde_json::Error>()
                .is_some_and(|error| error.is_io())
            || cause
                .downcast_ref::<reqwest::Error>()
                .is_some_and(|error| error.is_timeout() || error.is_body())
        {
            return network_error(error);
        }
        source = cause.source();
    }
    LlmError {
        code: "llm.invalid_response",
        detail: error.to_string(),
        retryable: false,
        retry_after: None,
    }
}

pub(super) fn max_serialized_body(
    body: &Value,
    max_bytes: usize,
    limit_name: &str,
) -> Result<(), LlmError> {
    let bytes = serde_json::to_vec(body).map_err(|error| LlmError {
        code: "llm.request_failed",
        detail: format!("Could not serialize {limit_name}: {error}"),
        retryable: false,
        retry_after: None,
    })?;
    if bytes.len() > max_bytes {
        return Err(LlmError {
            code: "llm.request_failed",
            detail: format!(
                "{limit_name} exceeds the serialized JSON body limit: {} bytes > {max_bytes} bytes. Reduce the image or prompt size; this request cannot be retried unchanged.",
                bytes.len()
            ),
            retryable: false,
        retry_after: None,
        });
    }
    Ok(())
}

pub(super) async fn response_status_error(
    response: reqwest::Response,
    status: StatusCode,
) -> LlmError {
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_retry_after(value, chrono::Utc::now()));
    tracing::warn!(
        http_status = status.as_u16(),
        retry_after_ms = retry_after.map(|delay| delay.as_millis() as u64),
        "LLM HTTP request failed"
    );
    let value = match response.text().await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
            json!({ "error": { "message": format!("HTTP {status} returned a non-JSON response") } })
        }),
        Err(error) => return network_error(error),
    };
    let mut error = status_error(status, &value);
    error.retry_after = retry_after;
    if status == StatusCode::PAYMENT_REQUIRED
        && retry_after.is_some()
        && value
            .pointer("/error/metadata/limit_source")
            .and_then(Value::as_str)
            == Some("openrouter_in_flight_budget")
    {
        error.code = "llm.provider_unavailable";
        error.retryable = true;
    }
    error
}

fn parse_retry_after(
    value: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<std::time::Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(std::time::Duration::from_secs(seconds));
    }
    chrono::DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|date| {
            (date.with_timezone(&chrono::Utc) - now)
                .to_std()
                .unwrap_or_default()
        })
}

pub(super) fn status_error(status: StatusCode, value: &Value) -> LlmError {
    let detail = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("LLM request failed")
        .to_owned();
    let error_type = value
        .pointer("/error/metadata/error_type")
        .or_else(|| value.pointer("/error/type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (code, retryable) = match error_type {
        "authentication" => ("llm.authentication_failed", false),
        "permission_denied" | "content_policy_violation" | "refusal" => {
            ("llm.permission_denied", false)
        }
        "payment_required" => ("llm.payment_required", false),
        "max_tokens_exceeded" => ("llm.output_length_exceeded", false),
        "invalid_request"
        | "invalid_prompt"
        | "context_length_exceeded"
        | "token_limit_exceeded"
        | "precondition_failed"
        | "payload_too_large"
        | "invalid_image"
        | "image_too_large"
        | "image_too_small"
        | "unsupported_image_format"
        | "image_not_found"
        | "image_download_failed" => ("llm.request_failed", false),
        "rate_limit_exceeded" => ("llm.rate_limited", true),
        "provider_overloaded" | "provider_unavailable" | "server" => {
            ("llm.provider_unavailable", true)
        }
        "timeout" => ("llm.timeout", true),
        _ => match status.as_u16() {
            401 => ("llm.authentication_failed", false),
            402 => ("llm.payment_required", false),
            403 => ("llm.permission_denied", false),
            404 if detail.to_ascii_lowercase().contains("model")
                || error_type.to_ascii_lowercase().contains("model") =>
            {
                ("llm.model_not_found", false)
            }
            404 => ("llm.path_not_found", false),
            408 => ("llm.timeout", true),
            429 => ("llm.rate_limited", true),
            500..=599 => ("llm.provider_unavailable", true),
            _ => ("llm.request_failed", false),
        },
    };
    LlmError {
        code,
        detail,
        retryable,
        retry_after: None,
    }
}

pub(super) fn completion_error(value: &Value) -> Option<LlmError> {
    let error = value
        .get("error")
        .filter(|error| !error.is_null())
        .or_else(|| {
            value
                .get("choices")?
                .as_array()?
                .iter()
                .find_map(|choice| choice.get("error").filter(|error| !error.is_null()))
        })?;
    let status = error
        .get("code")
        .and_then(Value::as_u64)
        .and_then(|code| u16::try_from(code).ok())
        .and_then(|code| StatusCode::from_u16(code).ok())
        .filter(|status| status.is_client_error() || status.is_server_error())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    Some(status_error(status, &json!({"error": error})))
}

fn network_error_code(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        return "llm.timeout";
    }
    let detail = error.to_string().to_ascii_lowercase();
    if detail.contains("certificate") || detail.contains("tls") || detail.contains("ssl") {
        "llm.tls_failed"
    } else if detail.contains("dns")
        || detail.contains("name or service not known")
        || detail.contains("failed to lookup")
    {
        "llm.dns_failed"
    } else if detail.contains("refused") {
        "llm.connection_refused"
    } else {
        "llm.network_failed"
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mimo_id_only_catalog_adds_only_verified_native_capabilities() {
        use axum::{routing::get, Json, Router};
        let app = Router::new().route(
            "/models",
            get(|| async {
                Json(json!({"data": [
                    {"id": "mimo-v2.6-flash"}, {"id": "mimo-v2.6-pro"},
                    {"id": "mimo-v2.6-pro-ultraspeed"}, {"id": "mimo-v2.5-asr"},
                    {"id": "mimo-future"}
                ]}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let catalog = list_model_catalog(client.get(&url), "mimo").await.unwrap();
        assert_eq!(catalog.models.len(), 5);
        for model in crate::providers::MIMO_CHAT_MODELS {
            let details = &catalog.model_details[*model];
            assert_eq!(details.input_modalities, ["text", "image"]);
            assert_eq!(details.output_modalities, ["text"]);
            assert_eq!(details.max_output_tokens, Some(131_072));
            assert_eq!(details.field_sources["input_modalities"], "builtin");
        }
        assert_eq!(
            catalog.model_details["mimo-v2.5-asr"].input_modalities,
            ["audio"]
        );
        assert!(!catalog.model_details.contains_key("mimo-future"));
        let custom = list_model_catalog(client.get(&url), "openai_compatible")
            .await
            .unwrap();
        assert!(custom.model_details.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn catalogs_preserve_native_facts_and_legacy_id_only_responses() {
        use axum::{routing::get, Json, Router};
        let app = Router::new().route("/models", get(|| async { Json(json!({"data": [
            {"id": "shared", "context_window": 32000, "max_output_tokens": 2048,
             "max_completion_tokens": 1024, "input_modalities": ["text", "image"], "output_modalities": ["text"],
             "effort": {"supported_levels": ["low", "high"], "default_level": "high"}},
            {"id": "unknown"}
        ]})) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let deepseek = list_model_catalog(client.get(&url), "deepseek")
            .await
            .unwrap();
        assert_eq!(deepseek.models, ["shared", "unknown"]);
        assert_eq!(
            deepseek.model_details["shared"].max_output_tokens,
            Some(2048)
        );
        assert_eq!(
            deepseek.model_details["shared"].input_modalities,
            ["text", "image"]
        );
        assert!(!deepseek.model_details.contains_key("unknown"));
        let groq = list_model_catalog(client.get(&url), "groq").await.unwrap();
        assert_eq!(groq.model_details["shared"].context_tokens, Some(32000));
        assert_eq!(groq.model_details["shared"].max_output_tokens, Some(1024));
        assert!(groq.model_details["shared"].input_modalities.is_empty());
        for provider in [
            "openai",
            "alibaba_cloud",
            "alibaba_token_plan",
            "openai_compatible",
        ] {
            let catalog = list_model_catalog(client.get(&url), provider)
                .await
                .unwrap();
            assert_eq!(catalog.models, ["shared", "unknown"]);
            assert!(catalog.model_details.is_empty());
        }
        server.abort();
    }

    #[test]
    fn classifies_openrouter_errors_and_retry_after() {
        for (status, error_type, code, retryable) in [
            (402, "", "llm.payment_required", false),
            (403, "", "llm.permission_denied", false),
            (502, "rate_limit_exceeded", "llm.rate_limited", true),
            (502, "invalid_request", "llm.request_failed", false),
            (
                502,
                "max_tokens_exceeded",
                "llm.output_length_exceeded",
                false,
            ),
            (
                502,
                "content_policy_violation",
                "llm.permission_denied",
                false,
            ),
        ] {
            let error = status_error(
                StatusCode::from_u16(status).unwrap(),
                &json!({"error": {"metadata": {"error_type": error_type}}}),
            );
            assert_eq!((error.code, error.retryable), (code, retryable));
        }
        let now = chrono::DateTime::parse_from_rfc2822("Wed, 21 Oct 2015 07:28:00 GMT")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            parse_retry_after("2", now),
            Some(std::time::Duration::from_secs(2))
        );
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2015 07:28:03 GMT", now),
            Some(std::time::Duration::from_secs(3))
        );
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2015 07:27:00 GMT", now),
            Some(std::time::Duration::ZERO)
        );
        assert_eq!(parse_retry_after("invalid", now), None);
    }

    #[tokio::test]
    async fn preserves_retry_after_on_http_errors() {
        let response = reqwest::Response::from(
            tokio_tungstenite::tungstenite::http::Response::builder()
                .status(429)
                .header("retry-after", "10")
                .body("rate limited")
                .unwrap(),
        );
        let error = response_status_error(response, StatusCode::TOO_MANY_REQUESTS).await;
        assert_eq!(error.retry_after, Some(std::time::Duration::from_secs(10)));
        assert_eq!(error.code, "llm.rate_limited");
    }

    #[tokio::test]
    async fn only_temporary_in_flight_budget_errors_with_a_valid_delay_can_retry() {
        for (limit_source, retry_after, expected) in [
            ("openrouter_in_flight_budget", Some("2"), true),
            ("openrouter_in_flight_budget", None, false),
            ("openrouter_in_flight_budget", Some("invalid"), false),
            ("openrouter_key_limit", Some("2"), false),
            ("", Some("2"), false),
        ] {
            let mut response =
                tokio_tungstenite::tungstenite::http::Response::builder().status(402);
            if let Some(delay) = retry_after {
                response = response.header("retry-after", delay);
            }
            let response = reqwest::Response::from(
                response
                    .body(
                        json!({"error": {"metadata": {"limit_source": limit_source}}}).to_string(),
                    )
                    .unwrap(),
            );
            let error = response_status_error(response, StatusCode::PAYMENT_REQUIRED).await;
            assert_eq!(error.retryable, expected);
        }
    }

    #[test]
    fn serialized_body_limit_counts_utf8_escaping_and_json_overhead() {
        let body = json!({"text": "你好\n\"\\", "image": "data:image/png;base64,cG5n"});
        let bytes = serde_json::to_vec(&body).unwrap();
        max_serialized_body(&body, bytes.len(), "Test request").unwrap();
        let error = max_serialized_body(&body, bytes.len() - 1, "Test request").unwrap_err();
        assert_eq!(error.code, "llm.request_failed");
        assert!(!error.retryable);
        assert!(error.detail.contains("serialized JSON body limit"));
        assert!(error.detail.contains("cannot be retried unchanged"));
        assert!(error.detail.contains(&bytes.len().to_string()));
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
    fn classifies_model_related_not_found_errors() {
        let error = status_error(
            StatusCode::NOT_FOUND,
            &json!({"error": {"message": "The model does not exist"}}),
        );

        assert_eq!(error.code, "llm.model_not_found");
    }

    #[test]
    fn preserves_path_not_found_for_missing_routes() {
        let error = status_error(
            StatusCode::NOT_FOUND,
            &json!({"error": {"message": "Route not found"}}),
        );

        assert_eq!(error.code, "llm.path_not_found");
    }
}
