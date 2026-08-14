use serde::Serialize;

use crate::config::{
    ApiAuthMode, ApiProfile, ALIBABA_PROVIDER, API_PURPOSE_ASR, API_PURPOSE_LLM,
    API_PURPOSE_SHARED, DEEPL_PROVIDER, GEMINI_PROVIDER, MICROSOFT_PROVIDER,
    OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER,
};

const TRANSLATION_LANGUAGES: &[&str] = &[
    "zh-Hans", "zh-Hant", "en", "ja", "ko", "es", "fr", "de", "ru",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    Native,
    ProtocolCompatible,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CapabilitySupportLevels {
    pub asr: Option<SupportLevel>,
    pub translation: Option<SupportLevel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderCapabilities {
    pub supports_streaming: bool,
    pub supports_model_listing: bool,
    pub requires_api_key: bool,
    pub is_local: bool,
    pub supports_context: bool,
    pub supports_translation: bool,
    pub supports_asr: bool,
    pub supported_languages: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub purposes: &'static [&'static str],
    pub support_levels: CapabilitySupportLevels,
    pub capabilities: ProviderCapabilities,
    pub presets: &'static [ProviderPreset],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub base_url: &'static str,
    pub auth_mode: ApiAuthMode,
    pub is_local: bool,
}

pub const OPENAI_COMPATIBLE_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "deepseek",
        display_name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        auth_mode: ApiAuthMode::Bearer,
        is_local: false,
    },
    ProviderPreset {
        id: "groq",
        display_name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        auth_mode: ApiAuthMode::Bearer,
        is_local: false,
    },
    ProviderPreset {
        id: "openrouter",
        display_name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        auth_mode: ApiAuthMode::Bearer,
        is_local: false,
    },
    ProviderPreset {
        id: "lm_studio",
        display_name: "LM Studio",
        base_url: "http://127.0.0.1:1234/v1",
        auth_mode: ApiAuthMode::None,
        is_local: true,
    },
    ProviderPreset {
        id: "ollama",
        display_name: "Ollama",
        base_url: "http://127.0.0.1:11434/v1",
        auth_mode: ApiAuthMode::None,
        is_local: true,
    },
    ProviderPreset {
        id: "custom",
        display_name: "Custom",
        base_url: "",
        auth_mode: ApiAuthMode::Bearer,
        is_local: false,
    },
];

pub fn catalog() -> Vec<ProviderDefinition> {
    [
        ALIBABA_PROVIDER,
        OPENAI_PROVIDER,
        GEMINI_PROVIDER,
        OPENAI_COMPATIBLE_PROVIDER,
        DEEPL_PROVIDER,
        MICROSOFT_PROVIDER,
    ]
    .into_iter()
    .filter_map(definition)
    .collect()
}

pub fn definition(provider: &str) -> Option<ProviderDefinition> {
    let llm = &[API_PURPOSE_LLM][..];
    let shared = &[API_PURPOSE_ASR, API_PURPOSE_LLM, API_PURPOSE_SHARED][..];
    let (id, display_name, purposes, support_levels, capabilities) = match provider {
        ALIBABA_PROVIDER => (
            ALIBABA_PROVIDER,
            "Alibaba Cloud",
            shared,
            CapabilitySupportLevels {
                asr: Some(SupportLevel::Native),
                translation: Some(SupportLevel::ProtocolCompatible),
            },
            capabilities(true, true, true, true),
        ),
        OPENAI_PROVIDER => (
            OPENAI_PROVIDER,
            "OpenAI",
            shared,
            CapabilitySupportLevels {
                asr: Some(SupportLevel::Native),
                translation: Some(SupportLevel::Native),
            },
            capabilities(false, true, true, true),
        ),
        GEMINI_PROVIDER => (
            GEMINI_PROVIDER,
            "Gemini",
            llm,
            CapabilitySupportLevels {
                asr: None,
                translation: Some(SupportLevel::Native),
            },
            capabilities(true, true, true, false),
        ),
        OPENAI_COMPATIBLE_PROVIDER => (
            OPENAI_COMPATIBLE_PROVIDER,
            "OpenAI Compatible",
            llm,
            CapabilitySupportLevels {
                asr: None,
                translation: Some(SupportLevel::ProtocolCompatible),
            },
            capabilities(true, true, true, false),
        ),
        DEEPL_PROVIDER => (
            DEEPL_PROVIDER,
            "DeepL",
            llm,
            CapabilitySupportLevels {
                asr: None,
                translation: Some(SupportLevel::Native),
            },
            capabilities(false, false, false, false),
        ),
        MICROSOFT_PROVIDER => (
            MICROSOFT_PROVIDER,
            "Microsoft Translator",
            llm,
            CapabilitySupportLevels {
                asr: None,
                translation: Some(SupportLevel::Native),
            },
            capabilities(false, false, false, false),
        ),
        _ => return None,
    };
    Some(ProviderDefinition {
        id,
        display_name,
        purposes,
        support_levels,
        capabilities,
        presets: if id == OPENAI_COMPATIBLE_PROVIDER {
            OPENAI_COMPATIBLE_PRESETS
        } else {
            &[]
        },
    })
}

pub fn profile_capabilities(profile: &ApiProfile) -> Option<ProviderCapabilities> {
    let mut result = definition(&profile.provider)?.capabilities;
    if profile.provider == OPENAI_COMPATIBLE_PROVIDER {
        result.requires_api_key = profile.requires_api_key();
        result.is_local = profile.is_local;
    }
    let purpose = profile.effective_purpose();
    result.supports_asr &= matches!(purpose, API_PURPOSE_ASR | API_PURPOSE_SHARED);
    result.supports_translation &= matches!(purpose, API_PURPOSE_LLM | API_PURPOSE_SHARED);
    result.supports_context &= result.supports_translation;
    result.supports_model_listing &= result.supports_translation;
    result.supports_streaming &= result.supports_translation;
    Some(result)
}

pub fn profile_support_levels(profile: &ApiProfile) -> Option<CapabilitySupportLevels> {
    let mut levels = definition(&profile.provider)?.support_levels;
    let purpose = profile.effective_purpose();
    if !matches!(purpose, API_PURPOSE_ASR | API_PURPOSE_SHARED) {
        levels.asr = None;
    }
    if !matches!(purpose, API_PURPOSE_LLM | API_PURPOSE_SHARED) {
        levels.translation = None;
    }
    Some(levels)
}

fn capabilities(
    supports_streaming: bool,
    supports_model_listing: bool,
    supports_context: bool,
    supports_asr: bool,
) -> ProviderCapabilities {
    ProviderCapabilities {
        supports_streaming,
        supports_model_listing,
        requires_api_key: true,
        is_local: false,
        supports_context,
        supports_translation: true,
        supports_asr,
        supported_languages: TRANSLATION_LANGUAGES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(provider: &str, purpose: &str) -> ApiProfile {
        ApiProfile {
            id: "profile".into(),
            name: "Profile".into(),
            provider: provider.into(),
            region: None,
            workspace_id: None,
            base_url: None,
            purpose: Some(purpose.into()),
            ..ApiProfile::default()
        }
    }

    #[test]
    fn gemini_is_native_llm_provider() {
        let definition = definition(GEMINI_PROVIDER).unwrap();
        assert_eq!(definition.purposes, &[API_PURPOSE_LLM]);
        assert_eq!(
            definition.support_levels.translation,
            Some(SupportLevel::Native)
        );
        assert!(definition.capabilities.supports_streaming);
        assert!(definition.capabilities.supports_model_listing);
        assert!(!definition.capabilities.supports_asr);
    }

    #[test]
    fn profile_purpose_limits_provider_capabilities() {
        let capabilities =
            profile_capabilities(&profile(ALIBABA_PROVIDER, API_PURPOSE_ASR)).unwrap();
        assert!(capabilities.supports_asr);
        assert!(!capabilities.supports_translation);
        assert!(!capabilities.supports_model_listing);
    }

    #[test]
    fn compatible_provider_exposes_presets_and_profile_auth_capabilities() {
        let definition = definition(OPENAI_COMPATIBLE_PROVIDER).unwrap();
        assert_eq!(definition.presets.len(), 6);
        assert!(definition.presets.iter().any(|preset| {
            preset.id == "ollama" && preset.auth_mode == ApiAuthMode::None && preset.is_local
        }));

        let mut local = profile(OPENAI_COMPATIBLE_PROVIDER, API_PURPOSE_LLM);
        local.auth_mode = ApiAuthMode::None;
        local.is_local = true;
        let capabilities = profile_capabilities(&local).unwrap();
        assert!(!capabilities.requires_api_key);
        assert!(capabilities.is_local);
    }
}
