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
                    gemini::generate(&self.http, api_key, request, on_progress).await
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
        match resolved.service.adapter {
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
