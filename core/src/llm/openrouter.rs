//! OpenRouter catalog metadata and its normalized reasoning controls.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{LlmError, http, openai_compatible};
use crate::config::ApiProfile;

pub use super::model_catalog::{ModelCatalog, ModelDetails, ReasoningOptions};

type CachedCatalog = (ApiProfile, Instant, Result<Arc<ModelCatalog>, LlmError>);

#[derive(Clone, Default)]
pub(super) struct CatalogCache(Arc<Mutex<BTreeMap<String, CachedCatalog>>>);

impl CatalogCache {
    pub async fn get(
        &self,
        http: &reqwest::Client,
        profile: &ApiProfile,
        api_key: &str,
    ) -> Result<Arc<ModelCatalog>, LlmError> {
        {
            let cache = self.0.lock().await;
            if let Some((saved_profile, updated, result)) = cache.get(&profile.id) {
                let ttl = if result.is_ok() { 300 } else { 60 };
                if saved_profile == profile && updated.elapsed() < Duration::from_secs(ttl) {
                    return result.clone();
                }
            }
        }
        let result = fetch_catalog(http, profile, api_key)
            .await
            .map(|mut catalog| {
                super::models_dev::supplement(profile, &mut catalog);
                Arc::new(catalog)
            });
        self.0.lock().await.insert(
            profile.id.clone(),
            (profile.clone(), Instant::now(), result.clone()),
        );
        result
    }
}

pub(super) async fn fetch_catalog(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
) -> Result<ModelCatalog, LlmError> {
    fetch_catalog_for_output(http, profile, api_key, "text").await
}

pub(crate) async fn fetch_transcription_catalog(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
) -> Result<ModelCatalog, LlmError> {
    fetch_catalog_for_output(http, profile, api_key, "transcription").await
}

async fn fetch_catalog_for_output(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    output: &str,
) -> Result<ModelCatalog, LlmError> {
    let mut request = http.get(openai_compatible::models_url(profile)?);
    if output != "text" {
        request = request.query(&[("output_modalities", output)]);
    }
    let response = openai_compatible::apply_profile_request(request, profile, api_key)?
        .send()
        .await
        .map_err(http::network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(http::response_status_error(response, status).await);
    }
    let value = response.json().await.map_err(http::invalid_response)?;
    let catalog = parse_catalog_for_output(&value, output);
    if catalog.models.is_empty() {
        return Err(LlmError {
            code: "llm.invalid_response",
            detail: format!("OpenRouter did not return any models with {output} output"),
            retryable: false,
            retry_after: None,
        });
    }
    Ok(catalog)
}

#[cfg(test)]
pub(super) fn parse_catalog(value: &Value) -> ModelCatalog {
    parse_catalog_for_output(value, "text")
}

fn parse_catalog_for_output(value: &Value, output: &str) -> ModelCatalog {
    let mut details = BTreeMap::new();
    for entry in value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !entry
            .pointer("/architecture/output_modalities")
            .and_then(Value::as_array)
            .is_some_and(|modalities| modalities.iter().any(|modality| modality == output))
        {
            continue;
        }
        let Some(id) = entry
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty() && id.len() <= 200 && !id.ends_with(":batch"))
        else {
            continue;
        };
        let mut info = ModelDetails {
            reasoning: entry
                .get("reasoning")
                .filter(|value| !value.is_null())
                .map(|value| {
                    let mut options: ReasoningOptions =
                        serde_json::from_value(value.clone()).unwrap_or_default();
                    // Null accepts all gateway efforts; an absent field exposes no selector.
                    if value.get("supported_efforts").is_some_and(Value::is_null) {
                        options.supported_efforts = Some(
                            ["max", "xhigh", "high", "medium", "low", "minimal", "none"]
                                .map(str::to_owned)
                                .to_vec(),
                        );
                    }
                    options.supported = Some(true);
                    options.can_disable = options.mandatory.map(|mandatory| !mandatory);
                    if let Some(values) = &options.supported_efforts {
                        options
                            .controls
                            .push(super::model_catalog::ReasoningControl::Effort {
                                values: values.clone(),
                            });
                    }
                    if options.supports_max_tokens == Some(true) {
                        options.controls.push(
                            super::model_catalog::ReasoningControl::BudgetTokens {
                                min: Some(1),
                                max: None,
                            },
                        );
                    }
                    options
                }),
            ..ModelDetails::from_provider(entry)
        };
        if info.reasoning.is_some() {
            info.field_sources
                .insert("reasoning".into(), "provider_api".into());
            for field in [
                "can_disable",
                "supports_max_tokens",
                "supported_efforts",
                "controls",
            ] {
                info.field_sources
                    .insert(format!("reasoning.{field}"), "provider_api".into());
            }
        }
        details.insert(id.to_owned(), info);
    }
    let mut models = details.keys().cloned().collect::<Vec<_>>();
    models.sort_by_key(|model| model.to_ascii_lowercase());
    ModelCatalog {
        models,
        model_details: details,
    }
}

pub(super) fn apply_reasoning(body: &mut Value, details: Option<&ModelDetails>, enabled: bool) {
    let details = details.filter(|details| {
        !details.stale
            || details
                .field_sources
                .get("reasoning")
                .is_some_and(|s| s == "provider_api")
    });
    let mut reasoning = json!({"exclude": true});
    match details.and_then(|details| details.reasoning.as_ref()) {
        Some(options) if !enabled && options.mandatory == Some(false) => {
            reasoning["enabled"] = json!(false)
        }
        Some(options) if !enabled && options.mandatory == Some(true) => {
            if let Some(effort) = options
                .supported_efforts
                .as_ref()
                .filter(|_| {
                    details.is_none_or(|details| {
                        !details.stale
                            || details
                                .field_sources
                                .get("reasoning.supported_efforts")
                                .is_none_or(|source| source != "models_dev")
                    })
                })
                .and_then(|efforts| {
                    ["minimal", "low", "medium", "high", "xhigh", "max"]
                        .into_iter()
                        .find(|candidate| efforts.iter().any(|effort| effort == candidate))
                })
            {
                reasoning["effort"] = json!(effort);
            }
        }
        _ if enabled => reasoning["enabled"] = json!(true),
        _ => {}
    }
    if details
        .and_then(|details| details.reasoning.as_ref())
        .is_some_and(|options| options.supported == Some(false))
    {
        return;
    }
    body["reasoning"] = reasoning;
}

pub fn needs_reasoning_budget(details: Option<&ModelDetails>, enabled: bool, model: &str) -> bool {
    let details = details.filter(|details| {
        !details.stale
            || details
                .field_sources
                .get("reasoning")
                .is_some_and(|s| s == "provider_api")
    });
    match details {
        Some(details) => {
            details.reasoning.as_ref().is_some_and(|options| {
                options.supported != Some(false) && (enabled || options.mandatory != Some(false))
            }) || model.starts_with("openrouter/")
        }
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn openrouter_transcription_catalog_requests_and_filters_stt_models() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let length = stream.read(&mut buffer).await.unwrap();
                assert!(length > 0);
                request.extend_from_slice(&buffer[..length]);
                if request.windows(4).any(|value| value == b"\r\n\r\n") {
                    break;
                }
            }
            let body = json!({"data": [
                {"id": "new-provider/new-asr", "architecture": {"input_modalities": ["audio"], "output_modalities": ["transcription"]}},
                {"id": "openai/whisper-1", "architecture": {"input_modalities": ["audio"], "output_modalities": ["transcription"]}},
                {"id": "audio-chat", "architecture": {"input_modalities": ["audio"], "output_modalities": ["text"]}},
                {"id": "audio-output", "architecture": {"output_modalities": ["audio"]}},
                {"id": "unknown"},
                {"id": "new-provider/new-asr", "architecture": {"output_modalities": ["transcription"]}}
            ]}).to_string();
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        let profile = ApiProfile {
            provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(format!("http://{address}/api/v1")),
            headers: vec![crate::config::HttpHeaderConfig {
                name: "X-Test".into(),
                value: "catalog".into(),
            }],
            ..ApiProfile::default()
        };
        let catalog = fetch_transcription_catalog(&crate::http_client(), &profile, "test-key")
            .await
            .unwrap();
        assert_eq!(catalog.models, ["new-provider/new-asr", "openai/whisper-1"]);
        assert_eq!(catalog.model_details.len(), 2);
        assert_eq!(
            catalog.model_details["openai/whisper-1"].input_modalities,
            ["audio"]
        );
        let request = server.await.unwrap();
        assert!(request.starts_with("GET /api/v1/models?output_modalities=transcription HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer test-key")
        );
        assert!(request.contains("x-test: catalog"));
    }

    #[test]
    fn openrouter_transcription_models_do_not_enter_the_text_catalog() {
        let catalog = parse_catalog(&json!({"data": [
            {"id": "stt", "architecture": {"output_modalities": ["transcription"]}},
            {"id": "text", "architecture": {"output_modalities": ["text"]}}
        ]}));
        assert_eq!(catalog.models, ["text"]);
    }

    #[test]
    fn provider_limits_and_output_modalities_are_preserved() {
        let catalog = parse_catalog(&json!({"data": [{
            "id": "text", "context_length": 32000,
            "top_provider": {"max_completion_tokens": 2048},
            "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}
        }]}));
        let details = serde_json::to_value(&catalog.model_details["text"]).unwrap();
        assert_eq!(details["max_output_tokens"], 2048);
        assert_eq!(details["output_modalities"], json!(["text"]));
    }

    #[test]
    fn null_effort_options_accept_gateway_levels_but_missing_options_do_not() {
        let catalog = parse_catalog(&json!({"data": [
            {"id": "all", "architecture": {"output_modalities": ["text"]}, "reasoning": {"mandatory": true, "supported_efforts": null}},
            {"id": "missing", "architecture": {"output_modalities": ["text"]}, "reasoning": {"mandatory": true}}
        ]}));
        let mut body = json!({});
        apply_reasoning(&mut body, catalog.model_details.get("all"), false);
        assert_eq!(body["reasoning"]["effort"], "minimal");
        apply_reasoning(&mut body, catalog.model_details.get("missing"), false);
        assert!(body["reasoning"].get("effort").is_none());
    }

    #[test]
    fn metadata_filters_modalities_without_losing_reasoning_options() {
        let catalog = parse_catalog(&json!({"data": [
            {"id": "text", "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}},
            {"id": "vision", "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]}, "reasoning": {"mandatory": true, "supported_efforts": ["high", "low", "minimal"], "future_field": true}},
            {"id": "image-output", "architecture": {"output_modalities": ["image"]}},
            {"id": "text:batch", "architecture": {"output_modalities": ["text"]}}
        ]}));
        assert_eq!(catalog.models, ["text", "vision"]);
        assert_eq!(
            catalog.model_details["vision"].input_modalities,
            ["text", "image"]
        );
        let mut body = json!({"max_tokens": 256});
        apply_reasoning(&mut body, catalog.model_details.get("vision"), false);
        assert_eq!(
            body["reasoning"],
            json!({"exclude": true, "effort": "minimal"})
        );
        assert_eq!(body["max_tokens"], 256);
        assert!(needs_reasoning_budget(
            catalog.model_details.get("vision"),
            false,
            "vision"
        ));
        assert!(!needs_reasoning_budget(
            catalog.model_details.get("text"),
            false,
            "text"
        ));
    }

    #[test]
    fn only_explicitly_optional_reasoning_is_disabled() {
        let optional = ModelDetails {
            reasoning: Some(ReasoningOptions {
                mandatory: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut body = json!({});
        apply_reasoning(&mut body, Some(&optional), false);
        assert_eq!(
            body["reasoning"],
            json!({"exclude": true, "enabled": false})
        );
        apply_reasoning(&mut body, None, false);
        assert_eq!(body["reasoning"], json!({"exclude": true}));
        apply_reasoning(&mut body, Some(&optional), true);
        assert_eq!(body["reasoning"], json!({"exclude": true, "enabled": true}));
        assert!(needs_reasoning_budget(None, false, "unknown"));
    }
}
