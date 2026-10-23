//! Provider-neutral LLM client facade.

use std::time::Instant;

use crate::config::{ApiProfile, ReasoningSetting};
use crate::providers::{
    self, OpenAiProtocolBehavior, ServiceAdapter, CAPABILITY_IMAGE_UNDERSTANDING,
    CAPABILITY_TEXT_GENERATION,
};

mod alibaba;
mod gemini;
mod http;
pub mod model_catalog;
pub mod models_dev;
mod openai;
mod openai_compatible;
pub mod openrouter;
mod reasoning;

#[derive(Debug, Clone)]
pub struct LlmRequest<'a> {
    pub model: &'a str,
    pub instructions: &'a str,
    pub input: &'a str,
    pub max_output_tokens: u32,
    pub thinking_enabled: bool,
    pub reasoning_control: Option<ReasoningSetting>,
}

pub type LlmProgress = dyn Fn(&str) + Send + Sync;

#[derive(Debug, Clone, PartialEq)]
pub struct LlmError {
    pub code: &'static str,
    pub detail: String,
    pub retryable: bool,
    pub retry_after: Option<std::time::Duration>,
}

#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    openrouter_catalog: openrouter::CatalogCache,
}

impl LlmClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            openrouter_catalog: Default::default(),
        }
    }

    pub async fn openrouter_models(
        &self,
        profile: &ApiProfile,
        api_key: &str,
    ) -> Result<std::sync::Arc<openrouter::ModelCatalog>, LlmError> {
        let mut catalog = self
            .openrouter_catalog
            .get(&self.http, profile, api_key)
            .await?;
        models_dev::supplement(profile, std::sync::Arc::make_mut(&mut catalog));
        Ok(catalog)
    }

    pub async fn openrouter_model_details(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        model: &str,
    ) -> Option<openrouter::ModelDetails> {
        if profile.provider != providers::OPENROUTER_PROVIDER {
            return None;
        }
        match self.openrouter_models(profile, api_key).await {
            Ok(catalog) => catalog.model_details.get(model).cloned(),
            Err(error) => {
                tracing::debug!(
                    error_code = error.code,
                    "OpenRouter model metadata unavailable"
                );
                models_dev::model_details(profile, model)
            }
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

    /// Generates a non-streaming text answer from text and PNG bytes.
    /// The caller validates the image. Adapters enforce known encoded request limits.
    /// `max_output_tokens` is a hard total generation limit, including thinking.
    pub async fn generate_with_image(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        request: LlmRequest<'_>,
        image: &[u8],
    ) -> Result<String, LlmError> {
        let resolved =
            providers::resolve_profile_capability(profile, CAPABILITY_IMAGE_UNDERSTANDING)
                .map_err(|detail| LlmError {
                    code: "llm.unsupported_provider",
                    detail,
                    retryable: false,
                    retry_after: None,
                })?;
        if image.is_empty() {
            return Err(LlmError {
                code: "llm.request_failed",
                detail: "Image input must not be empty".into(),
                retryable: false,
                retry_after: None,
            });
        }
        let details = self
            .openrouter_model_details(profile, api_key, request.model)
            .await
            .or_else(|| models_dev::model_details(profile, request.model));
        if details.as_ref().is_some_and(|details| {
            details
                .field_sources
                .get("input_modalities")
                .is_some_and(|source| source == "provider_api")
                && !details.input_modalities.is_empty()
                && !details
                    .input_modalities
                    .iter()
                    .any(|modality| modality == "image")
        }) {
            return Err(LlmError {
                code: "llm.request_failed",
                detail: format!(
                    "The selected {} model does not accept image input",
                    resolved.provider.display_name
                ),
                retryable: false,
                retry_after: None,
            });
        }
        let started = Instant::now();
        let model = request.model;
        let max_output_tokens = request.max_output_tokens;
        let result = match resolved.service.adapter {
            ServiceAdapter::OpenAiResponses => {
                openai::generate_with_image(&self.http, profile, api_key, request, image).await
            }
            ServiceAdapter::GeminiGenerateContent => {
                gemini::generate_with_image(&self.http, profile, api_key, request, image).await
            }
            ServiceAdapter::OpenAiChatCompletions { behavior } => {
                openai_compatible::generate_with_image(
                    &self.http,
                    profile,
                    api_key,
                    request,
                    image,
                    resolved.provider.display_name,
                    behavior,
                    details.as_ref(),
                )
                .await
            }
            ServiceAdapter::AlibabaChatCompletions => {
                openai_compatible::generate_with_image(
                    &self.http,
                    profile,
                    api_key,
                    request,
                    image,
                    resolved.provider.display_name,
                    OpenAiProtocolBehavior::Alibaba,
                    None,
                )
                .await
            }
            adapter => Err(unsupported_adapter(adapter)),
        };
        tracing::info!(
            provider = profile.provider,
            model,
            capability = CAPABILITY_IMAGE_UNDERSTANDING,
            latency_ms = started.elapsed().as_millis() as u64,
            max_output_tokens,
            success = result.is_ok(),
            error_code = result.as_ref().err().map(|error| error.code),
            retryable = result.as_ref().err().map(|error| error.retryable),
            "LLM image request completed"
        );
        result
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
        let max_output_tokens = request.max_output_tokens;
        let details = self
            .openrouter_model_details(profile, api_key, request.model)
            .await;
        let result = match providers::resolve_profile_capability(profile, capability) {
            Ok(resolved) => match resolved.service.adapter {
                ServiceAdapter::OpenAiResponses => {
                    openai::generate(&self.http, profile, api_key, request).await
                }
                ServiceAdapter::OpenAiChatCompletions { behavior } => {
                    openai_compatible::generate(
                        &self.http,
                        profile,
                        api_key,
                        request,
                        resolved.provider.display_name,
                        behavior,
                        details.as_ref(),
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
                retry_after: None,
            }),
        };
        tracing::info!(
            provider = profile.provider,
            capability,
            model,
            latency_ms = started.elapsed().as_millis() as u64,
            input_chars,
            output_chars = result.as_ref().map_or(0, |text| text.chars().count()),
            thinking_enabled,
            streamed = on_progress.is_some(),
            success = result.is_ok(),
            max_output_tokens,
            error_code = result.as_ref().err().map(|error| error.code),
            retryable = result.as_ref().err().map(|error| error.retryable),
            retry_after_ms = result
                .as_ref()
                .err()
                .and_then(|error| error.retry_after)
                .map(|delay| delay.as_millis() as u64),
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
            CAPABILITY_IMAGE_UNDERSTANDING,
        ]
        .into_iter()
        .find_map(|capability| providers::resolve_profile_capability(profile, capability).ok())
        .ok_or_else(|| LlmError {
            code: "llm.models_unsupported",
            detail: format!(
                "API profile {} has not enabled an LLM capability",
                profile.id
            ),
            retryable: false,
            retry_after: None,
        })?;
        if !resolved.service.supports_model_listing {
            return Err(LlmError {
                code: "llm.models_unsupported",
                detail: format!("Service {} does not expose LLM models", resolved.service.id),
                retryable: false,
                retry_after: None,
            });
        }
        self.list_models_with_adapter(profile, api_key, resolved.service.adapter)
            .await
            .map(|models| providers::compatible_service_models(resolved.service, models))
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
            retry_after: None,
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
                retry_after: None,
            })?;
        self.list_models_with_adapter(profile, api_key, adapter)
            .await
    }

    pub async fn list_model_catalog(
        &self,
        profile: &ApiProfile,
        api_key: &str,
    ) -> Result<model_catalog::ModelCatalog, LlmError> {
        if profile.provider == providers::OPENROUTER_PROVIDER {
            return self
                .openrouter_models(profile, api_key)
                .await
                .map(|catalog| catalog.as_ref().clone());
        }
        let mut catalog = match profile.provider.as_str() {
            providers::GEMINI_PROVIDER => gemini::list_model_catalog(&self.http, api_key).await?,
            providers::OPENAI_PROVIDER => {
                openai::list_model_catalog(&self.http, profile, api_key).await?
            }
            providers::GROQ_PROVIDER | providers::DEEPSEEK_PROVIDER | providers::MIMO_PROVIDER => {
                openai_compatible::list_model_catalog(&self.http, profile, api_key).await?
            }
            _ => model_catalog::ModelCatalog {
                models: self.list_provider_models(profile, api_key).await?,
                ..Default::default()
            },
        };
        models_dev::supplement(profile, &mut catalog);
        for (model, details) in &mut catalog.model_details {
            reasoning::add_builtin_options(details, &profile.provider, model);
        }
        Ok(catalog)
    }

    async fn list_models_with_adapter(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        adapter: ServiceAdapter,
    ) -> Result<Vec<String>, LlmError> {
        if profile.provider == providers::OPENROUTER_PROVIDER {
            return self
                .openrouter_models(profile, api_key)
                .await
                .map(|catalog| catalog.models.clone());
        }
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
            retry_after: None,
        })?;
        let ServiceAdapter::OpenAiChatCompletions { behavior } = resolved.service.adapter else {
            return Err(LlmError {
                code: "llm.models_unsupported",
                detail: "Strict streaming diagnostics require an OpenAI-compatible adapter".into(),
                retryable: false,
                retry_after: None,
            });
        };
        let details = self
            .openrouter_model_details(profile, api_key, request.model)
            .await;
        openai_compatible::test_streaming(
            &self.http,
            profile,
            api_key,
            request,
            resolved.provider.display_name,
            behavior,
            details.as_ref(),
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
        retry_after: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiAuthMode, HttpHeaderConfig};
    use axum::{http::HeaderMap, routing::post, Json, Router};
    use serde_json::{json, Value};

    fn request() -> LlmRequest<'static> {
        LlmRequest {
            model: "vision-model",
            instructions: "Answer briefly",
            input: "What is visible?",
            max_output_tokens: 256,
            thinking_enabled: false,
            reasoning_control: None,
        }
    }

    #[tokio::test]
    async fn native_provider_image_constraints_apply_only_to_direct_facts() {
        let gemini = ApiProfile {
            id: "native-gemini-prefix-fixture".into(),
            provider: providers::GEMINI_PROVIDER.into(),
            ..Default::default()
        };
        let mut catalog = model_catalog::ModelCatalog {
            models: vec!["prefix-fixture".into()],
            model_details: [(
                "prefix-fixture".into(),
                model_catalog::ModelDetails::from_native(
                    &json!({"outputTokenLimit": 256}),
                    "gemini",
                ),
            )]
            .into(),
        };
        models_dev::supplement(&gemini, &mut catalog);
        assert_eq!(
            models_dev::model_details(&gemini, " models/prefix-fixture ")
                .unwrap()
                .max_output_tokens,
            Some(256)
        );
        let profile = ApiProfile {
            id: "native-image-constraint-fixture".into(),
            provider: providers::DEEPSEEK_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_IMAGE_UNDERSTANDING.into()],
            ..Default::default()
        };
        let mut catalog = model_catalog::ModelCatalog {
            models: vec!["vision-model".into()],
            model_details: [(
                "vision-model".into(),
                model_catalog::ModelDetails::from_native(
                    &json!({"input_modalities": ["text"]}),
                    "deepseek",
                ),
            )]
            .into(),
        };
        models_dev::supplement(&profile, &mut catalog);
        // A local closed proxy prevents any external API request if validation regresses.
        let http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .build()
            .unwrap();
        let client = LlmClient::new(http);
        let error = client
            .generate_with_image(&profile, "fixture", request(), b"image")
            .await
            .unwrap_err();
        assert_eq!(error.code, "llm.request_failed");
        assert!(error.detail.contains("does not accept image input"));
        let mut public = profile.clone();
        public.id = "public-image-constraint-fixture".into();
        catalog
            .model_details
            .get_mut("vision-model")
            .unwrap()
            .field_sources
            .insert("input_modalities".into(), "models_dev".into());
        models_dev::supplement(&public, &mut catalog);
        let error = client
            .generate_with_image(&public, "fixture", request(), b"image")
            .await
            .unwrap_err();
        assert!(!error.detail.contains("does not accept image input"));
    }

    #[tokio::test]
    async fn image_and_text_requests_preserve_profile_connection_and_text_behavior() {
        for auth_mode in [ApiAuthMode::Bearer, ApiAuthMode::None] {
            for with_image in [true, false] {
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let sender = std::sync::Arc::new(std::sync::Mutex::new(Some(sender)));
                let app = Router::new().route(
                    "/custom/v1/chat/completions",
                    post(
                        move |headers: HeaderMap, Json(body): Json<Value>| async move {
                            sender
                                .lock()
                                .unwrap()
                                .take()
                                .unwrap()
                                .send((headers, body))
                                .unwrap();
                            Json(json!({"choices": [{"message": {"content": "Visible answer"}}]}))
                        },
                    ),
                );
                let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                    .await
                    .unwrap();
                let address = listener.local_addr().unwrap();
                let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
                let profile = ApiProfile {
                    provider: providers::OPENAI_COMPATIBLE_PROVIDER.into(),
                    enabled_capabilities: vec![if with_image {
                        CAPABILITY_IMAGE_UNDERSTANDING.into()
                    } else {
                        CAPABILITY_TEXT_GENERATION.into()
                    }],
                    base_url: Some(format!("http://{address}/custom/v1/chat/completions")),
                    auth_mode,
                    timeout_ms: 1000,
                    headers: vec![HttpHeaderConfig {
                        name: "X-Test-Client".into(),
                        value: "VRCS".into(),
                    }],
                    ..ApiProfile::default()
                };
                let client = LlmClient::new(reqwest::Client::new());
                let output = if with_image {
                    client
                        .generate_with_image(&profile, "test-key", request(), b"\x89PNG\r\n\x1a\n")
                        .await
                } else {
                    client.generate(&profile, "test-key", request(), None).await
                }
                .unwrap();
                let (headers, body) = receiver.await.unwrap();
                server.abort();
                assert_eq!(output, "Visible answer");
                assert_eq!(headers["x-test-client"], "VRCS");
                if auth_mode == ApiAuthMode::Bearer {
                    assert_eq!(headers["authorization"], "Bearer test-key");
                } else {
                    assert!(!headers.contains_key("authorization"));
                }
                assert_eq!(body["model"], "vision-model");
                assert_eq!(body["max_tokens"], 256);
                assert_eq!(body["stream"], false);
                assert_eq!(
                    body["messages"][0],
                    json!({"role": "system", "content": "Answer briefly"})
                );
                assert_eq!(body["messages"][1]["role"], "user");
                if with_image {
                    assert_eq!(
                        body["messages"][1]["content"],
                        json!([
                            {"type": "text", "text": "What is visible?"},
                            {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}}
                        ])
                    );
                } else {
                    assert_eq!(body["messages"][1]["content"], "What is visible?");
                }
            }
        }
    }

    #[tokio::test]
    async fn rejects_disabled_unsupported_and_empty_image_inputs_before_http() {
        let client = LlmClient::new(reqwest::Client::new());
        let mut profile = ApiProfile {
            provider: providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_TEXT_GENERATION.into()],
            ..ApiProfile::default()
        };
        let error = client
            .generate_with_image(&profile, "", request(), b"png")
            .await
            .unwrap_err();
        assert_eq!(error.code, "llm.unsupported_provider");
        assert!(!error.retryable);
        profile.enabled_capabilities = vec![CAPABILITY_IMAGE_UNDERSTANDING.into()];
        for provider in providers::catalog() {
            if provider.capabilities.supports_image_understanding {
                continue;
            }
            profile.provider = provider.id.into();
            let error = client
                .generate_with_image(&profile, "", request(), b"png")
                .await
                .unwrap_err();
            assert_eq!(error.code, "llm.unsupported_provider");
            assert!(!error.retryable);
        }
        for provider in providers::catalog() {
            if !provider.capabilities.supports_image_understanding {
                continue;
            }
            profile.provider = provider.id.into();
            let error = client
                .generate_with_image(&profile, "", request(), &[])
                .await
                .unwrap_err();
            assert_eq!(error.code, "llm.request_failed", "{}", provider.id);
            assert!(!error.retryable);
        }
    }

    #[tokio::test]
    async fn image_only_profile_can_list_models() {
        let app = Router::new().route(
            "/v1/models",
            axum::routing::get(|| async { Json(json!({"data": [{"id": "vision-model"}]})) }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let profile = ApiProfile {
            provider: providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_IMAGE_UNDERSTANDING.into()],
            base_url: Some(format!("http://{address}/v1")),
            auth_mode: ApiAuthMode::None,
            ..ApiProfile::default()
        };
        let models = LlmClient::new(reqwest::Client::new())
            .list_models(&profile, "")
            .await
            .unwrap();
        server.abort();
        assert_eq!(models, ["vision-model"]);
    }
}
