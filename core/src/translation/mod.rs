//! 字幕翻译编排。专业翻译 API 在这里适配；通用 LLM 调用委托给 `llm` 模块。

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::config::{ApiProfile, TranslationConfig};
use crate::credentials;
use crate::llm::{LlmClient, LlmProgress, LlmRequest};
use crate::models::{now_iso8601, SubtitleTranslation};
use crate::providers::{
    self, ALIBABA_PROVIDER, DEEPL_PROVIDER, GEMINI_PROVIDER, MICROSOFT_PROVIDER,
    OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER,
};

mod deepl;
mod dispatcher;
mod glossary_subscription;
mod microsoft;
mod prompt;

pub use dispatcher::TranslationDispatcher;
pub use glossary_subscription::{GlossarySubscriptionError, GlossarySubscriptionStore};
pub use prompt::{TranslationContextEntry, TranslationPromptBuilder};

#[derive(Debug, Clone, PartialEq)]
pub struct TranslationError {
    pub code: &'static str,
    pub detail: String,
    pub retryable: bool,
}

#[derive(Debug, Clone)]
pub struct TranslationResult {
    pub text: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub provider: String,
    pub model: Option<String>,
}

impl TranslationResult {
    pub fn into_record(self) -> SubtitleTranslation {
        SubtitleTranslation {
            text: self.text,
            source_language: self.source_language,
            target_language: self.target_language,
            provider: self.provider,
            model: self.model,
            created_at: now_iso8601(),
        }
    }
}

#[derive(Clone)]
pub struct TranslationService {
    http: reqwest::Client,
    llm: LlmClient,
    glossary_subscription: Option<Arc<GlossarySubscriptionStore>>,
}

impl TranslationService {
    #[cfg(test)]
    pub fn new() -> Result<Self, String> {
        Self::build(None)
    }

    pub fn with_glossary_subscription(
        glossary_subscription: Arc<GlossarySubscriptionStore>,
    ) -> Result<Self, String> {
        Self::build(Some(glossary_subscription))
    }

    fn build(
        glossary_subscription: Option<Arc<GlossarySubscriptionStore>>,
    ) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|error| format!("Failed to create translation HTTP client: {error}"))?;
        Ok(Self {
            llm: LlmClient::new(http.clone()),
            http,
            glossary_subscription,
        })
    }

    pub async fn translate(
        &self,
        settings: &TranslationConfig,
        profiles: &[ApiProfile],
        text: &str,
        source_language: Option<&str>,
        target_override: Option<&str>,
        context: &[TranslationContextEntry],
    ) -> Result<TranslationResult, TranslationError> {
        self.translate_with_progress(
            settings,
            profiles,
            text,
            source_language,
            target_override,
            context,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn translate_with_progress(
        &self,
        settings: &TranslationConfig,
        profiles: &[ApiProfile],
        text: &str,
        source_language: Option<&str>,
        target_override: Option<&str>,
        context: &[TranslationContextEntry],
        on_progress: Option<&LlmProgress>,
    ) -> Result<TranslationResult, TranslationError> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > 5_000 {
            return Err(error(
                "translation.invalid_text",
                "Translation text must contain between 1 and 5000 characters",
                false,
            ));
        }
        let target = target_override.unwrap_or(&settings.target_language);
        if !providers::is_valid_translation_language(target) {
            return Err(error(
                "translation.invalid_target_language",
                format!("Invalid translation target language: {target}"),
                false,
            ));
        }
        let profile_id = settings.profile_id.as_deref().ok_or_else(|| {
            error(
                "translation.not_configured",
                "No translation API profile is selected",
                false,
            )
        })?;
        let profile = profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| {
                error(
                    "translation.not_configured",
                    "The selected translation API profile does not exist",
                    false,
                )
            })?;
        if !providers::supports_translation_language(profile, target) {
            return Err(error(
                "translation.invalid_target_language",
                format!(
                    "The selected translation provider does not support target language: {target}"
                ),
                false,
            ));
        }
        if source_language.is_some_and(|source| same_translation_language(source, target)) {
            return Ok(TranslationResult {
                text: text.to_owned(),
                source_language: source_language.map(str::to_owned),
                target_language: target.to_owned(),
                provider: "local".into(),
                model: None,
            });
        }
        let api_key = if profile.requires_api_key() {
            credentials::read_credential(&profile.id, &profile.provider)
                .map_err(|detail| error("translation.credential_failed", detail, false))?
                .ok_or_else(|| {
                    error(
                        "translation.credential_missing",
                        "The selected translation API profile has no API key",
                        false,
                    )
                })?
        } else {
            String::new()
        };

        match profile.provider.as_str() {
            DEEPL_PROVIDER => {
                deepl::translate(&self.http, profile, &api_key, text, source_language, target).await
            }
            MICROSOFT_PROVIDER => {
                microsoft::translate(&self.http, profile, &api_key, text, source_language, target)
                    .await
            }
            OPENAI_PROVIDER | OPENAI_COMPATIBLE_PROVIDER | ALIBABA_PROVIDER | GEMINI_PROVIDER => {
                self.llm(
                    profile,
                    &api_key,
                    &settings.model,
                    settings.thinking_enabled,
                    text,
                    source_language,
                    target,
                    &settings.prompt,
                    context,
                    on_progress,
                )
                .await
            }
            provider => Err(error(
                "translation.unsupported_provider",
                format!("Unsupported translation provider: {provider}"),
                false,
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn llm(
        &self,
        profile: &ApiProfile,
        api_key: &str,
        model: &str,
        thinking_enabled: bool,
        text: &str,
        source: Option<&str>,
        target: &str,
        prompt_config: &crate::config::TranslationPromptConfig,
        context: &[TranslationContextEntry],
        on_progress: Option<&LlmProgress>,
    ) -> Result<TranslationResult, TranslationError> {
        let resolved_prompt;
        let prompt_config = if let Some(subscription) = &self.glossary_subscription {
            resolved_prompt = subscription.merged_prompt(prompt_config);
            &resolved_prompt
        } else {
            prompt_config
        };
        let prompt =
            TranslationPromptBuilder::new(prompt_config).build(source, target, context, text);
        let translated = self
            .llm
            .generate(
                profile,
                api_key,
                LlmRequest {
                    model,
                    instructions: &prompt.instructions,
                    input: &prompt.input,
                    max_output_tokens: translation_output_token_limit(text),
                    thinking_enabled,
                },
                on_progress,
            )
            .await
            .map_err(|error| TranslationError {
                code: error.code,
                detail: error.detail,
                retryable: error.retryable,
            })?;
        Ok(TranslationResult {
            text: translated,
            source_language: source.map(str::to_owned),
            target_language: target.to_owned(),
            provider: profile.provider.clone(),
            model: Some(model.to_owned()),
        })
    }
}

fn translation_output_token_limit(text: &str) -> u32 {
    let estimated = text.chars().count().saturating_mul(2).saturating_add(64);
    estimated.clamp(128, 8_192) as u32
}

pub fn same_translation_language(source: &str, target: &str) -> bool {
    let source = source.to_ascii_lowercase();
    let target = target.to_ascii_lowercase();
    if source == target {
        return true;
    }
    if target.starts_with("zh-") {
        return matches!(
            (source.as_str(), target.as_str()),
            ("zh-cn" | "zh-hans", "zh-hans") | ("zh-tw" | "zh-hant", "zh-hant")
        );
    }
    if !target.contains('-') {
        return source.split('-').next() == Some(target.as_str());
    }
    false
}

pub(super) fn error(
    code: &'static str,
    detail: impl Into<String>,
    retryable: bool,
) -> TranslationError {
    TranslationError {
        code,
        detail: detail.into(),
        retryable,
    }
}

pub(super) fn invalid(detail: impl Into<String>) -> TranslationError {
    error("translation.invalid_response", detail, false)
}

pub(super) fn network_error(source: reqwest::Error) -> TranslationError {
    error(
        if source.is_timeout() {
            "translation.timeout"
        } else {
            "translation.network_failed"
        },
        source.to_string(),
        true,
    )
}

pub(super) fn invalid_response(error: reqwest::Error) -> TranslationError {
    invalid(error.to_string())
}

pub(super) fn http_error(status: reqwest::StatusCode, value: &Value) -> TranslationError {
    let detail = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Translation request failed")
        .to_owned();
    match status.as_u16() {
        401 | 403 => error("translation.authentication_failed", detail, false),
        408 => error("translation.timeout", detail, true),
        429 => error("translation.rate_limited", detail, true),
        456 => error("translation.quota_exceeded", detail, false),
        500..=599 => error("translation.provider_unavailable", detail, true),
        _ => error("translation.request_failed", detail, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_language_codes_by_script_and_base_language() {
        assert!(!same_translation_language("zh", "zh-Hant"));
        assert!(same_translation_language("zh-CN", "zh-Hans"));
        assert!(!same_translation_language("ja", "en"));
        assert!(same_translation_language("en-US", "en"));
        assert!(!same_translation_language("en-US", "en-GB"));
        assert!(!same_translation_language("pt-BR", "pt-PT"));
    }

    #[test]
    fn translation_output_limit_scales_without_unbounded_generation() {
        assert_eq!(translation_output_token_limit("hello"), 128);
        assert_eq!(translation_output_token_limit(&"あ".repeat(200)), 464);
        assert_eq!(translation_output_token_limit(&"あ".repeat(5_000)), 8_192);
    }
}
