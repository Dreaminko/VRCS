//! Provider-neutral LLM client facade.

use std::time::Instant;

use crate::config::ApiProfile;
use crate::providers::{self, ServiceAdapter, CAPABILITY_TEXT_GENERATION};

mod alibaba;
mod gemini;
mod http;
mod openai;
mod openai_compatible;
mod reasoning;

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
    #[cfg(test)]
    alignment_endpoint: Option<String>,
}

impl LlmClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            #[cfg(test)]
            alignment_endpoint: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_alignment_endpoint(endpoint: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            alignment_endpoint: Some(endpoint),
        }
    }

    /// Alignment uses its selected profile without enqueuing a translation.
    pub(crate) async fn align_native_translation(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        request: LlmRequest<'_>,
        schema: serde_json::Value,
    ) -> Result<String, LlmError> {
        #[cfg(test)]
        if let Some(endpoint) = &self.alignment_endpoint {
            return openai::generate_structured_at(&self.http, api_key, request, schema, endpoint)
                .await;
        }
        let provider = providers::definition(&profile.provider).ok_or_else(|| LlmError {
            code: "llm.unsupported_provider",
            detail: format!("Unsupported API provider: {}", profile.provider),
            retryable: false,
        })?;
        // A recognition-only profile may share its credential with the provider's text model.
        let adapter = provider
            .services
            .iter()
            .find_map(|service| match service.adapter {
                adapter @ (ServiceAdapter::OpenAiResponses
                | ServiceAdapter::OpenAiChatCompletions { .. }
                | ServiceAdapter::AlibabaChatCompletions
                | ServiceAdapter::GeminiGenerateContent) => Some(adapter),
                _ => None,
            })
            .ok_or_else(|| LlmError {
                code: "llm.unsupported_provider",
                detail: format!("Provider {} does not expose a text model", profile.provider),
                retryable: false,
            })?;
        if matches!(adapter, ServiceAdapter::OpenAiResponses) {
            return openai::generate_structured(&self.http, api_key, request, schema).await;
        }
        // Keep other adapters unchanged; validate their JSON mapping before applying it.
        let instructions = format!(
            "{}\nReturn ONLY a JSON object matching this JSON Schema, without Markdown or explanation:\n{}",
            request.instructions, schema
        );
        let request = LlmRequest {
            instructions: &instructions,
            ..request
        };
        match adapter {
            ServiceAdapter::OpenAiChatCompletions { behavior } => {
                openai_compatible::generate(
                    &self.http,
                    profile,
                    api_key,
                    request,
                    provider.display_name,
                    behavior,
                    None,
                )
                .await
            }
            ServiceAdapter::AlibabaChatCompletions => {
                alibaba::generate(&self.http, profile, api_key, request, None).await
            }
            ServiceAdapter::GeminiGenerateContent => {
                gemini::generate(&self.http, profile, api_key, request, None).await
            }
            adapter => Err(unsupported_adapter(adapter)),
        }
    }

    pub async fn generate(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        request: LlmRequest<'_>,
        on_progress: Option<&LlmProgress>,
    ) -> Result<String, LlmError> {
        self.generate_for_capability(
            profile,
            api_key,
            CAPABILITY_TEXT_GENERATION,
            request,
            on_progress,
        )
        .await
    }

    pub(crate) async fn generate_for_capability(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        capability: &str,
        request: LlmRequest<'_>,
        on_progress: Option<&LlmProgress>,
    ) -> Result<String, LlmError> {
        let started = Instant::now();
        let model = request.model.to_owned();
        let input_chars = request.input.chars().count();
        let thinking_enabled = request.thinking_enabled;
        let result = match providers::resolve_profile_capability(profile, capability) {
            Ok(resolved) => match resolved.service.adapter {
                ServiceAdapter::OpenAiResponses => {
                    openai::generate(&self.http, api_key, request).await
                }
                ServiceAdapter::OpenAiChatCompletions { behavior } => {
                    openai_compatible::generate(
                        &self.http,
                        profile,
                        api_key,
                        request,
                        resolved.provider.display_name,
                        behavior,
                        on_progress,
                    )
                    .await
                }
                ServiceAdapter::AlibabaChatCompletions => {
                    alibaba::generate(&self.http, profile, api_key, request, on_progress).await
                }
                ServiceAdapter::GeminiGenerateContent => {
                    gemini::generate(&self.http, profile, api_key, request, on_progress).await
                }
                adapter => Err(unsupported_adapter(adapter)),
            },
            Err(detail) => Err(LlmError {
                code: "llm.unsupported_provider",
                detail,
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
        let resolved = [
            CAPABILITY_TEXT_GENERATION,
            providers::CAPABILITY_TEXT_TRANSLATION,
        ]
        .into_iter()
        .find_map(|capability| providers::resolve_profile_capability(profile, capability).ok())
        .ok_or_else(|| LlmError {
            code: "llm.models_unsupported",
            detail: format!(
                "API profile {} has not enabled an LLM text capability",
                profile.id
            ),
            retryable: false,
        })?;
        if !resolved.service.supports_model_listing {
            return Err(LlmError {
                code: "llm.models_unsupported",
                detail: format!("Service {} does not expose LLM models", resolved.service.id),
                retryable: false,
            });
        }
        self.list_models_with_adapter(profile, api_key, resolved.service.adapter)
            .await
    }

    pub async fn list_provider_models(
        &self,
        profile: &ApiProfile,
        api_key: &str,
    ) -> Result<Vec<String>, LlmError> {
        let provider = providers::definition(&profile.provider).ok_or_else(|| LlmError {
            code: "llm.models_unsupported",
            detail: format!("Unsupported API provider: {}", profile.provider),
            retryable: false,
        })?;
        let adapter = provider
            .services
            .iter()
            .find(|service| {
                service.supports_model_listing
                    && matches!(
                        service.adapter,
                        ServiceAdapter::AlibabaChatCompletions
                            | ServiceAdapter::OpenAiResponses
                            | ServiceAdapter::OpenAiChatCompletions { .. }
                            | ServiceAdapter::GeminiGenerateContent
                    )
            })
            .map(|service| service.adapter)
            .ok_or_else(|| LlmError {
                code: "llm.models_unsupported",
                detail: format!(
                    "Provider {} does not expose a model catalog",
                    profile.provider
                ),
                retryable: false,
            })?;
        self.list_models_with_adapter(profile, api_key, adapter)
            .await
    }

    async fn list_models_with_adapter(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        adapter: ServiceAdapter,
    ) -> Result<Vec<String>, LlmError> {
        match adapter {
            ServiceAdapter::OpenAiResponses => {
                openai::list_models(&self.http, profile, api_key).await
            }
            ServiceAdapter::OpenAiChatCompletions { .. } => {
                openai_compatible::list_models(&self.http, profile, api_key).await
            }
            ServiceAdapter::AlibabaChatCompletions => {
                alibaba::list_models(&self.http, profile, api_key).await
            }
            ServiceAdapter::GeminiGenerateContent => gemini::list_models(&self.http, api_key).await,
            adapter => Err(unsupported_adapter(adapter)),
        }
    }

    pub async fn test_openai_compatible_streaming(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        request: LlmRequest<'_>,
        on_progress: &LlmProgress,
    ) -> Result<String, LlmError> {
        let resolved = providers::resolve_profile_capability(profile, CAPABILITY_TEXT_GENERATION)
            .map_err(|detail| LlmError {
            code: "llm.models_unsupported",
            detail,
            retryable: false,
        })?;
        let ServiceAdapter::OpenAiChatCompletions { behavior } = resolved.service.adapter else {
            return Err(LlmError {
                code: "llm.models_unsupported",
                detail: "Strict streaming diagnostics require an OpenAI-compatible adapter".into(),
                retryable: false,
            });
        };
        openai_compatible::test_streaming(
            &self.http,
            profile,
            api_key,
            request,
            resolved.provider.display_name,
            behavior,
            on_progress,
        )
        .await
    }
}

fn unsupported_adapter(adapter: ServiceAdapter) -> LlmError {
    LlmError {
        code: "llm.unsupported_provider",
        detail: format!("Unsupported LLM service adapter: {adapter:?}"),
        retryable: false,
    }
}

#[cfg(test)]
mod alignment_tests {
    use super::*;
    use crate::config::{ApiAuthMode, HttpHeaderConfig};
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use serde_json::{json, Value};

    #[tokio::test]
    async fn alignment_uses_saved_provider_endpoint_auth_headers_and_unrestricted_model() {
        for (provider, auth_mode) in [
            (providers::OPENAI_COMPATIBLE_PROVIDER, ApiAuthMode::Bearer),
            (providers::OLLAMA_PROVIDER, ApiAuthMode::None),
            (providers::LM_STUDIO_PROVIDER, ApiAuthMode::None),
        ] {
            let (sent, mut received) = tokio::sync::mpsc::channel(1);
            let app = Router::new()
                .route(
                    "/v1/chat/completions",
                    post(
                        |State(sent): State<tokio::sync::mpsc::Sender<(HeaderMap, Value)>>,
                         headers: HeaderMap,
                         Json(body): Json<Value>| async move {
                            sent.send((headers, body)).await.unwrap();
                            Json(json!({"choices":[{"message":{"content":"{\"groups\":[]}"}}]}))
                        },
                    ),
                )
                .with_state(sent);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let profile = ApiProfile {
                id: "saved".into(),
                provider: provider.into(),
                base_url: Some(format!("http://{address}/v1")),
                auth_mode,
                headers: vec![HttpHeaderConfig {
                    name: "X-Alignment-Test".into(),
                    value: "saved-header".into(),
                }],
                ..ApiProfile::default()
            };
            let text = LlmClient::new(reqwest::Client::new())
                .align_native_translation(
                    &profile,
                    "test-key",
                    LlmRequest {
                        model: "user-chosen-model",
                        instructions: "Align the streams.",
                        input: "transcripts",
                        max_output_tokens: 2048,
                        thinking_enabled: false,
                    },
                    json!({"type":"object","properties":{"groups":{"type":"array"}}}),
                )
                .await
                .unwrap();
            assert_eq!(text, "{\"groups\":[]}");
            let (headers, body) = received.recv().await.unwrap();
            assert_eq!(headers["x-alignment-test"], "saved-header");
            if auth_mode == ApiAuthMode::Bearer {
                assert_eq!(headers["authorization"], "Bearer test-key");
            } else {
                assert!(!headers.contains_key("authorization"));
            }
            assert_eq!(body["model"], "user-chosen-model");
            assert_eq!(body["stream"], false);
            assert_eq!(body["messages"][1]["content"], "transcripts");
            let instructions = body["messages"][0]["content"].as_str().unwrap();
            assert!(instructions.contains("Align the streams."));
            assert!(instructions.contains("JSON Schema"));
            assert!(instructions.contains("groups"));
            server.abort();
        }
    }
}
