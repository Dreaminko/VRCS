use serde::Serialize;

use crate::config::{ApiAuthMode, ApiProfile};

mod validation;

pub(crate) use validation::validate_profile;

pub const ALIBABA_PROVIDER: &str = "alibaba_cloud";
pub const OPENAI_PROVIDER: &str = "openai";
pub const OPENAI_COMPATIBLE_PROVIDER: &str = "openai_compatible";
pub const GEMINI_PROVIDER: &str = "gemini";
pub const DEEPL_PROVIDER: &str = "deepl";
pub const MICROSOFT_PROVIDER: &str = "microsoft_translator";
pub const API_PURPOSE_ASR: &str = "asr";
pub const API_PURPOSE_LLM: &str = "llm";
pub const API_PURPOSE_SHARED: &str = "shared";

pub const LLM_TRANSLATION_LANGUAGES: &[&str] = &[
    "zh-Hans", "zh-Hant", "yue-Hant", "en", "ja", "ko", "es", "fr", "de", "ru", "ar", "bg", "cs",
    "da", "el", "he", "hi", "id", "it", "ms", "nb", "nl", "pl", "pt-BR", "pt-PT", "ro", "sv", "th",
    "tr", "uk", "vi", "fil", "hu", "fi",
];

const DEEPL_TRANSLATION_LANGUAGES: &[&str] = &[
    "zh-Hans", "zh-Hant", "en", "ja", "ko", "es", "fr", "de", "ru", "ar", "bg", "cs", "da", "el",
    "he", "id", "it", "nb", "nl", "pl", "pt-BR", "pt-PT", "ro", "sv", "th", "tr", "uk", "vi", "hu",
    "fi",
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
    pub supports_text_generation: bool,
    pub supports_translation: bool,
    pub supports_asr: bool,
    pub supports_custom_translation_language: bool,
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
            capabilities(true, true, true, true, LLM_TRANSLATION_LANGUAGES, true),
        ),
        OPENAI_PROVIDER => (
            OPENAI_PROVIDER,
            "OpenAI",
            shared,
            CapabilitySupportLevels {
                asr: Some(SupportLevel::Native),
                translation: Some(SupportLevel::Native),
            },
            capabilities(false, true, true, true, LLM_TRANSLATION_LANGUAGES, true),
        ),
        GEMINI_PROVIDER => (
            GEMINI_PROVIDER,
            "Gemini",
            llm,
            CapabilitySupportLevels {
                asr: None,
                translation: Some(SupportLevel::Native),
            },
            capabilities(true, true, true, false, LLM_TRANSLATION_LANGUAGES, true),
        ),
        OPENAI_COMPATIBLE_PROVIDER => (
            OPENAI_COMPATIBLE_PROVIDER,
            "OpenAI Compatible",
            llm,
            CapabilitySupportLevels {
                asr: None,
                translation: Some(SupportLevel::ProtocolCompatible),
            },
            capabilities(true, true, true, false, LLM_TRANSLATION_LANGUAGES, true),
        ),
        DEEPL_PROVIDER => (
            DEEPL_PROVIDER,
            "DeepL",
            llm,
            CapabilitySupportLevels {
                asr: None,
                translation: Some(SupportLevel::Native),
            },
            capabilities(
                false,
                false,
                false,
                false,
                DEEPL_TRANSLATION_LANGUAGES,
                false,
            ),
        ),
        MICROSOFT_PROVIDER => (
            MICROSOFT_PROVIDER,
            "Microsoft Translator",
            llm,
            CapabilitySupportLevels {
                asr: None,
                translation: Some(SupportLevel::Native),
            },
            capabilities(false, false, false, false, LLM_TRANSLATION_LANGUAGES, false),
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
    let purpose = effective_purpose(profile);
    result.supports_asr &= matches!(purpose, API_PURPOSE_ASR | API_PURPOSE_SHARED);
    result.supports_translation &= matches!(purpose, API_PURPOSE_LLM | API_PURPOSE_SHARED);
    result.supports_text_generation &= matches!(purpose, API_PURPOSE_LLM | API_PURPOSE_SHARED);
    result.supports_context &= result.supports_translation;
    result.supports_model_listing &= result.supports_translation;
    result.supports_streaming &= result.supports_translation;
    Some(result)
}

pub fn profile_support_levels(profile: &ApiProfile) -> Option<CapabilitySupportLevels> {
    let mut levels = definition(&profile.provider)?.support_levels;
    let purpose = effective_purpose(profile);
    if !matches!(purpose, API_PURPOSE_ASR | API_PURPOSE_SHARED) {
        levels.asr = None;
    }
    if !matches!(purpose, API_PURPOSE_LLM | API_PURPOSE_SHARED) {
        levels.translation = None;
    }
    Some(levels)
}

pub fn effective_purpose(profile: &ApiProfile) -> &str {
    profile.purpose.as_deref().unwrap_or_else(|| {
        if profile.provider == OPENAI_COMPATIBLE_PROVIDER
            || profile.provider == GEMINI_PROVIDER
            || matches!(
                profile.provider.as_str(),
                DEEPL_PROVIDER | MICROSOFT_PROVIDER
            )
        {
            API_PURPOSE_LLM
        } else {
            API_PURPOSE_SHARED
        }
    })
}

pub fn supports_realtime_asr(profile: &ApiProfile) -> bool {
    profile_capabilities(profile).is_some_and(|value| value.supports_asr)
}

pub fn supports_translation(profile: &ApiProfile) -> bool {
    profile_capabilities(profile).is_some_and(|value| value.supports_translation)
}

pub fn supports_text_generation(profile: &ApiProfile) -> bool {
    profile_capabilities(profile).is_some_and(|value| value.supports_text_generation)
}

pub fn supports_llm_models(profile: &ApiProfile) -> bool {
    profile_capabilities(profile).is_some_and(|value| value.supports_model_listing)
}

pub fn supports_translation_language(profile: &ApiProfile, language: &str) -> bool {
    profile_capabilities(profile).is_some_and(|capabilities| {
        capabilities.supports_translation
            && (capabilities.supports_custom_translation_language
                || capabilities.supported_languages.contains(&language))
    })
}

pub fn is_valid_translation_language(language: &str) -> bool {
    if !(2..=35).contains(&language.len()) {
        return false;
    }
    let mut subtags = language.split('-');
    let Some(primary) = subtags.next() else {
        return false;
    };
    primary.len() >= 2
        && primary.len() <= 8
        && primary.bytes().all(|value| value.is_ascii_alphabetic())
        && subtags.all(|subtag| {
            (2..=8).contains(&subtag.len())
                && subtag.bytes().all(|value| value.is_ascii_alphanumeric())
        })
}

pub fn translation_language_name(language: &str) -> Option<&'static str> {
    Some(match language {
        "zh-Hans" => "Chinese (Simplified)",
        "zh-Hant" => "Chinese (Traditional)",
        "yue-Hant" => "Cantonese (Traditional)",
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "ru" => "Russian",
        "ar" => "Arabic",
        "bg" => "Bulgarian",
        "cs" => "Czech",
        "da" => "Danish",
        "el" => "Greek",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "id" => "Indonesian",
        "it" => "Italian",
        "ms" => "Malay",
        "nb" => "Norwegian Bokmål",
        "nl" => "Dutch",
        "pl" => "Polish",
        "pt-BR" => "Portuguese (Brazil)",
        "pt-PT" => "Portuguese (Portugal)",
        "ro" => "Romanian",
        "sv" => "Swedish",
        "th" => "Thai",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "vi" => "Vietnamese",
        "fil" => "Filipino",
        "hu" => "Hungarian",
        "fi" => "Finnish",
        _ => return None,
    })
}

fn capabilities(
    supports_streaming: bool,
    supports_model_listing: bool,
    supports_context: bool,
    supports_asr: bool,
    supported_languages: &'static [&'static str],
    supports_custom_translation_language: bool,
) -> ProviderCapabilities {
    ProviderCapabilities {
        supports_streaming,
        supports_model_listing,
        requires_api_key: true,
        is_local: false,
        supports_context,
        supports_text_generation: supports_context,
        supports_translation: true,
        supports_asr,
        supports_custom_translation_language,
        supported_languages,
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
        assert!(definition.capabilities.supports_text_generation);
        assert!(!definition.capabilities.supports_asr);
    }

    #[test]
    fn profile_purpose_limits_provider_capabilities() {
        let capabilities =
            profile_capabilities(&profile(ALIBABA_PROVIDER, API_PURPOSE_ASR)).unwrap();
        assert!(capabilities.supports_asr);
        assert!(!capabilities.supports_translation);
        assert!(!capabilities.supports_text_generation);
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

    #[test]
    fn translation_language_capabilities_are_provider_specific() {
        let llm = profile(OPENAI_PROVIDER, API_PURPOSE_LLM);
        let deepl = profile(DEEPL_PROVIDER, API_PURPOSE_LLM);

        assert!(supports_text_generation(&llm));
        assert!(!supports_text_generation(&deepl));
        assert!(supports_translation_language(&llm, "tlh-Latn"));
        assert!(supports_translation_language(&deepl, "pt-BR"));
        assert!(!supports_translation_language(&deepl, "hi"));
        assert!(is_valid_translation_language("yue-Hant"));
        assert!(!is_valid_translation_language("en-u-ca-gregory"));
        assert!(!is_valid_translation_language("not a language"));
    }
}
