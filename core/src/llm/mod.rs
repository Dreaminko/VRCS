//! Provider-neutral LLM client facade.

use std::time::Instant;

use crate::config::ApiProfile;
use crate::providers::{
    ALIBABA_PROVIDER, GEMINI_PROVIDER, OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER,
};

mod alibaba;
mod gemini;
mod http;
mod openai;
mod openai_compatible;

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
        let started = Instant::now();
        let model = request.model.to_owned();
        let input_chars = request.input.chars().count();
        let thinking_enabled = request.thinking_enabled;
        let result = match profile.provider.as_str() {
            OPENAI_PROVIDER => openai::generate(&self.http, api_key, request).await,
            OPENAI_COMPATIBLE_PROVIDER => {
                openai_compatible::generate(&self.http, profile, api_key, request, on_progress)
                    .await
            }
            ALIBABA_PROVIDER => {
                alibaba::generate(&self.http, profile, api_key, request, on_progress).await
            }
            GEMINI_PROVIDER => gemini::generate(&self.http, api_key, request, on_progress).await,
            provider => Err(LlmError {
                code: "llm.unsupported_provider",
                detail: format!("Unsupported LLM provider: {provider}"),
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
        match profile.provider.as_str() {
            OPENAI_PROVIDER => openai::list_models(&self.http, profile, api_key).await,
            OPENAI_COMPATIBLE_PROVIDER => {
                openai_compatible::list_models(&self.http, profile, api_key).await
            }
            ALIBABA_PROVIDER => alibaba::list_models(&self.http, profile, api_key).await,
            GEMINI_PROVIDER => gemini::list_models(&self.http, api_key).await,
            provider => Err(LlmError {
                code: "llm.models_unsupported",
                detail: format!("Provider {provider} does not expose LLM models"),
                retryable: false,
            }),
        }
    }

    pub async fn test_openai_compatible_streaming(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        request: LlmRequest<'_>,
        on_progress: &LlmProgress,
    ) -> Result<String, LlmError> {
        if profile.provider != OPENAI_COMPATIBLE_PROVIDER {
            return Err(LlmError {
                code: "llm.models_unsupported",
                detail: "Strict streaming diagnostics require an OpenAI-compatible profile".into(),
                retryable: false,
            });
        }
        openai_compatible::test_streaming(&self.http, profile, api_key, request, on_progress).await
    }
}
