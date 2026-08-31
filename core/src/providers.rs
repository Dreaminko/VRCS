use serde::Serialize;

use crate::config::{ApiAuthMode, ApiProfile};

mod validation;

pub(crate) use validation::validate_profile;

pub const ALIBABA_PROVIDER: &str = "alibaba_cloud";
pub const ALIBABA_TOKEN_PLAN_PROVIDER: &str = "alibaba_token_plan";
pub const OPENAI_PROVIDER: &str = "openai";
pub const OPENAI_COMPATIBLE_PROVIDER: &str = "openai_compatible";
pub const GEMINI_PROVIDER: &str = "gemini";
pub const DEEPL_PROVIDER: &str = "deepl";
pub const MICROSOFT_PROVIDER: &str = "microsoft_translator";
pub const GROQ_PROVIDER: &str = "groq";
pub const OPENROUTER_PROVIDER: &str = "openrouter";
pub const DEEPSEEK_PROVIDER: &str = "deepseek";
pub const LM_STUDIO_PROVIDER: &str = "lm_studio";
pub const OLLAMA_PROVIDER: &str = "ollama";

pub const API_PURPOSE_ASR: &str = "asr";
pub const API_PURPOSE_LLM: &str = "llm";
pub const API_PURPOSE_SHARED: &str = "shared";

pub const CAPABILITY_SPEECH_TO_TEXT: &str = "speech_to_text";
pub const CAPABILITY_TEXT_GENERATION: &str = "text_generation";
pub const CAPABILITY_TEXT_TRANSLATION: &str = "text_translation";

pub const SERVICE_QWEN_REALTIME: &str = "qwen_realtime";
pub const SERVICE_FUN_ASR_REALTIME: &str = "fun_asr_realtime";
pub const SERVICE_TOKEN_PLAN_REALTIME: &str = "token_plan_realtime";
pub const SERVICE_OPENAI_REALTIME: &str = "openai_realtime";
pub const SERVICE_GROQ_TRANSCRIPTION: &str = "groq_transcription";
pub const SERVICE_GEMINI_TRANSCRIBE: &str = "gemini_transcribe";

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

const TEXT_CAPABILITIES: &[&str] = &[CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION];
const TRANSLATION_CAPABILITY: &[&str] = &[CAPABILITY_TEXT_TRANSLATION];
const SPEECH_CAPABILITY: &[&str] = &[CAPABILITY_SPEECH_TO_TEXT];
const LLM_PURPOSES: &[&str] = &[API_PURPOSE_LLM];
const SHARED_PURPOSES: &[&str] = &[API_PURPOSE_ASR, API_PURPOSE_LLM, API_PURPOSE_SHARED];
const OPENAI_MODELS: &[&str] = &[];
const QWEN_MODELS: &[&str] = &["qwen3-asr-flash-realtime"];
const FUN_ASR_MODELS: &[&str] = &["qwen-audio-3.0-asr-flash-streaming", "fun-asr-realtime"];
const TOKEN_PLAN_REALTIME_MODELS: &[&str] = &["qwen-audio-3.0-realtime-plus"];
const OPENAI_ASR_MODELS: &[&str] = &["gpt-4o-mini-transcribe", "gpt-4o-transcribe"];
const GROQ_ASR_MODELS: &[&str] = &["whisper-large-v3-turbo", "whisper-large-v3"];
const GEMINI_ASR_MODELS: &[&str] = &["gemini-3.5-transcribe-live"];

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

#[derive(Debug, Clone, Copy, Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCategory {
    CloudProvider,
    LocalService,
    CustomProtocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum BaseUrlPolicy {
    Fixed(&'static str),
    Regional,
    Editable(&'static str),
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProviderConnectionDefinition {
    pub auth_mode: ApiAuthMode,
    pub base_url: BaseUrlPolicy,
    pub environment_variables: &'static [&'static str],
    pub legacy_environment_variables: &'static [&'static str],
    pub allow_custom_headers: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiProtocolBehavior {
    Standard,
    Alibaba,
    DeepSeek,
    Groq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceAdapter {
    AlibabaChatCompletions,
    OpenAiResponses,
    OpenAiChatCompletions { behavior: OpenAiProtocolBehavior },
    GeminiGenerateContent,
    DeepLTextTranslation,
    MicrosoftTextTranslation,
    QwenRealtime,
    AlibabaTokenPlanRealtime,
    FunAsrRealtime,
    OpenAiRealtime,
    GeminiTranscribe,
    OpenAiAudioTranscriptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionTransport {
    RealtimeStream,
    SegmentedUpload,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProviderServiceDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub capabilities: &'static [&'static str],
    pub adapter: ServiceAdapter,
    pub recognition_transport: Option<RecognitionTransport>,
    pub partial_results: bool,
    pub supports_streaming: bool,
    pub supports_model_listing: bool,
    pub supports_context: bool,
    pub models: &'static [&'static str],
    pub context_max_chars: Option<usize>,
    pub asr_support: Option<SupportLevel>,
    pub translation_support: Option<SupportLevel>,
}

#[derive(Debug, Clone, Copy)]
struct RecognitionServiceSpec {
    transport: RecognitionTransport,
    partial_results: bool,
    models: &'static [&'static str],
    context_max_chars: Option<usize>,
    support: SupportLevel,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProviderDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub category: ProviderCategory,
    pub connection: ProviderConnectionDefinition,
    pub services: &'static [ProviderServiceDefinition],
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

pub const OPENAI_COMPATIBLE_PRESETS: &[ProviderPreset] = &[ProviderPreset {
    id: "custom",
    display_name: "Custom",
    base_url: "",
    auth_mode: ApiAuthMode::Bearer,
    is_local: false,
}];

const ALIBABA_SERVICES: &[ProviderServiceDefinition] = &[
    text_service(
        "alibaba_chat_completions",
        "Alibaba Chat Completions",
        ServiceAdapter::AlibabaChatCompletions,
        SupportLevel::ProtocolCompatible,
    ),
    recognition_service_definition(
        SERVICE_QWEN_REALTIME,
        "Qwen Realtime",
        ServiceAdapter::QwenRealtime,
        RecognitionServiceSpec {
            transport: RecognitionTransport::RealtimeStream,
            partial_results: true,
            models: QWEN_MODELS,
            context_max_chars: Some(2_000),
            support: SupportLevel::Native,
        },
    ),
    recognition_service_definition(
        SERVICE_FUN_ASR_REALTIME,
        "Qwen Audio / Fun-ASR Realtime",
        ServiceAdapter::FunAsrRealtime,
        RecognitionServiceSpec {
            transport: RecognitionTransport::RealtimeStream,
            partial_results: true,
            models: FUN_ASR_MODELS,
            context_max_chars: Some(400),
            support: SupportLevel::Native,
        },
    ),
];

const ALIBABA_TOKEN_PLAN_SERVICES: &[ProviderServiceDefinition] = &[
    text_service(
        "alibaba_token_plan_chat_completions",
        "Token Plan Chat Completions",
        ServiceAdapter::OpenAiChatCompletions {
            behavior: OpenAiProtocolBehavior::Alibaba,
        },
        SupportLevel::ProtocolCompatible,
    ),
    recognition_service_definition(
        SERVICE_TOKEN_PLAN_REALTIME,
        "Token Plan Realtime ASR",
        ServiceAdapter::AlibabaTokenPlanRealtime,
        RecognitionServiceSpec {
            transport: RecognitionTransport::RealtimeStream,
            partial_results: true,
            models: TOKEN_PLAN_REALTIME_MODELS,
            context_max_chars: None,
            support: SupportLevel::Native,
        },
    ),
];

const OPENAI_SERVICES: &[ProviderServiceDefinition] = &[
    text_service(
        "openai_responses",
        "OpenAI Responses",
        ServiceAdapter::OpenAiResponses,
        SupportLevel::Native,
    ),
    recognition_service_definition(
        SERVICE_OPENAI_REALTIME,
        "OpenAI Realtime",
        ServiceAdapter::OpenAiRealtime,
        RecognitionServiceSpec {
            transport: RecognitionTransport::RealtimeStream,
            partial_results: true,
            models: OPENAI_ASR_MODELS,
            context_max_chars: None,
            support: SupportLevel::Native,
        },
    ),
];

const GEMINI_SERVICES: &[ProviderServiceDefinition] = &[
    text_service(
        "gemini_generate_content",
        "Gemini Generate Content",
        ServiceAdapter::GeminiGenerateContent,
        SupportLevel::Native,
    ),
    recognition_service_definition(
        SERVICE_GEMINI_TRANSCRIBE,
        "Gemini Transcribe",
        ServiceAdapter::GeminiTranscribe,
        RecognitionServiceSpec {
            transport: RecognitionTransport::RealtimeStream,
            partial_results: true,
            models: GEMINI_ASR_MODELS,
            context_max_chars: Some(4_000),
            support: SupportLevel::Native,
        },
    ),
];

const CUSTOM_SERVICES: &[ProviderServiceDefinition] = &[text_service(
    "custom_openai_chat_completions",
    "OpenAI-compatible Chat Completions",
    ServiceAdapter::OpenAiChatCompletions {
        behavior: OpenAiProtocolBehavior::Standard,
    },
    SupportLevel::ProtocolCompatible,
)];

const DEEPL_SERVICES: &[ProviderServiceDefinition] = &[translation_service(
    "deepl_text_translation",
    "DeepL Text Translation",
    ServiceAdapter::DeepLTextTranslation,
)];

const MICROSOFT_SERVICES: &[ProviderServiceDefinition] = &[translation_service(
    "microsoft_text_translation",
    "Microsoft Text Translation",
    ServiceAdapter::MicrosoftTextTranslation,
)];

const GROQ_SERVICES: &[ProviderServiceDefinition] = &[
    text_service(
        "groq_chat_completions",
        "Groq Chat Completions",
        ServiceAdapter::OpenAiChatCompletions {
            behavior: OpenAiProtocolBehavior::Groq,
        },
        SupportLevel::ProtocolCompatible,
    ),
    recognition_service_definition(
        SERVICE_GROQ_TRANSCRIPTION,
        "Groq Transcription",
        ServiceAdapter::OpenAiAudioTranscriptions,
        RecognitionServiceSpec {
            transport: RecognitionTransport::SegmentedUpload,
            partial_results: false,
            models: GROQ_ASR_MODELS,
            context_max_chars: Some(400),
            support: SupportLevel::Native,
        },
    ),
];

const OPENROUTER_SERVICES: &[ProviderServiceDefinition] = &[text_service(
    "openrouter_chat_completions",
    "OpenRouter Chat Completions",
    ServiceAdapter::OpenAiChatCompletions {
        behavior: OpenAiProtocolBehavior::Standard,
    },
    SupportLevel::ProtocolCompatible,
)];

const DEEPSEEK_SERVICES: &[ProviderServiceDefinition] = &[text_service(
    "deepseek_chat_completions",
    "DeepSeek Chat Completions",
    ServiceAdapter::OpenAiChatCompletions {
        behavior: OpenAiProtocolBehavior::DeepSeek,
    },
    SupportLevel::Native,
)];

const LM_STUDIO_SERVICES: &[ProviderServiceDefinition] = &[text_service(
    "lm_studio_chat_completions",
    "LM Studio Chat Completions",
    ServiceAdapter::OpenAiChatCompletions {
        behavior: OpenAiProtocolBehavior::Standard,
    },
    SupportLevel::ProtocolCompatible,
)];

const OLLAMA_SERVICES: &[ProviderServiceDefinition] = &[text_service(
    "ollama_chat_completions",
    "Ollama Chat Completions",
    ServiceAdapter::OpenAiChatCompletions {
        behavior: OpenAiProtocolBehavior::Standard,
    },
    SupportLevel::ProtocolCompatible,
)];

const fn text_service(
    id: &'static str,
    display_name: &'static str,
    adapter: ServiceAdapter,
    translation_support: SupportLevel,
) -> ProviderServiceDefinition {
    ProviderServiceDefinition {
        id,
        display_name,
        capabilities: TEXT_CAPABILITIES,
        adapter,
        recognition_transport: None,
        partial_results: false,
        supports_streaming: true,
        supports_model_listing: true,
        supports_context: true,
        models: OPENAI_MODELS,
        context_max_chars: None,
        asr_support: None,
        translation_support: Some(translation_support),
    }
}

const fn translation_service(
    id: &'static str,
    display_name: &'static str,
    adapter: ServiceAdapter,
) -> ProviderServiceDefinition {
    ProviderServiceDefinition {
        id,
        display_name,
        capabilities: TRANSLATION_CAPABILITY,
        adapter,
        recognition_transport: None,
        partial_results: false,
        supports_streaming: false,
        supports_model_listing: false,
        supports_context: false,
        models: &[],
        context_max_chars: None,
        asr_support: None,
        translation_support: Some(SupportLevel::Native),
    }
}

const fn recognition_service_definition(
    id: &'static str,
    display_name: &'static str,
    adapter: ServiceAdapter,
    spec: RecognitionServiceSpec,
) -> ProviderServiceDefinition {
    ProviderServiceDefinition {
        id,
        display_name,
        capabilities: SPEECH_CAPABILITY,
        adapter,
        recognition_transport: Some(spec.transport),
        partial_results: spec.partial_results,
        supports_streaming: matches!(spec.transport, RecognitionTransport::RealtimeStream),
        supports_model_listing: matches!(
            adapter,
            ServiceAdapter::QwenRealtime
                | ServiceAdapter::AlibabaTokenPlanRealtime
                | ServiceAdapter::FunAsrRealtime
        ),
        supports_context: spec.context_max_chars.is_some(),
        models: spec.models,
        context_max_chars: spec.context_max_chars,
        asr_support: Some(spec.support),
        translation_support: None,
    }
}

pub fn catalog() -> Vec<ProviderDefinition> {
    [
        ALIBABA_PROVIDER,
        ALIBABA_TOKEN_PLAN_PROVIDER,
        OPENAI_PROVIDER,
        GROQ_PROVIDER,
        OPENROUTER_PROVIDER,
        DEEPSEEK_PROVIDER,
        GEMINI_PROVIDER,
        DEEPL_PROVIDER,
        MICROSOFT_PROVIDER,
        LM_STUDIO_PROVIDER,
        OLLAMA_PROVIDER,
        OPENAI_COMPATIBLE_PROVIDER,
    ]
    .into_iter()
    .filter_map(definition)
    .collect()
}

pub fn definition(provider: &str) -> Option<ProviderDefinition> {
    let (display_name, category, connection, services, purposes, languages, custom_languages) =
        match provider {
            ALIBABA_PROVIDER => (
                "Alibaba Cloud",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Regional,
                    &["VRCS_QWEN_API_KEY", "DASHSCOPE_API_KEY"],
                    &[],
                    false,
                ),
                ALIBABA_SERVICES,
                SHARED_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            ALIBABA_TOKEN_PLAN_PROVIDER => (
                "Alibaba Cloud Token Plan",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Fixed(
                        "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
                    ),
                    &["VRCS_ALIBABA_TOKEN_PLAN_API_KEY"],
                    &[],
                    false,
                ),
                ALIBABA_TOKEN_PLAN_SERVICES,
                SHARED_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            OPENAI_PROVIDER => (
                "OpenAI",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Fixed("https://api.openai.com/v1"),
                    &["VRCS_OPENAI_API_KEY", "OPENAI_API_KEY"],
                    &[],
                    false,
                ),
                OPENAI_SERVICES,
                SHARED_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            GROQ_PROVIDER => (
                "Groq",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Fixed("https://api.groq.com/openai/v1"),
                    &["VRCS_GROQ_API_KEY", "GROQ_API_KEY"],
                    &["VRCS_OPENAI_COMPATIBLE_API_KEY"],
                    true,
                ),
                GROQ_SERVICES,
                SHARED_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            OPENROUTER_PROVIDER => (
                "OpenRouter",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Fixed("https://openrouter.ai/api/v1"),
                    &["VRCS_OPENROUTER_API_KEY", "OPENROUTER_API_KEY"],
                    &["VRCS_OPENAI_COMPATIBLE_API_KEY"],
                    true,
                ),
                OPENROUTER_SERVICES,
                LLM_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            DEEPSEEK_PROVIDER => (
                "DeepSeek",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Fixed("https://api.deepseek.com/v1"),
                    &["VRCS_DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY"],
                    &["VRCS_OPENAI_COMPATIBLE_API_KEY"],
                    true,
                ),
                DEEPSEEK_SERVICES,
                LLM_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            GEMINI_PROVIDER => (
                "Gemini",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Fixed("https://generativelanguage.googleapis.com/v1beta"),
                    &["VRCS_GEMINI_API_KEY", "GEMINI_API_KEY"],
                    &[],
                    false,
                ),
                GEMINI_SERVICES,
                SHARED_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            DEEPL_PROVIDER => (
                "DeepL",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Fixed("https://api.deepl.com/v2"),
                    &["VRCS_DEEPL_API_KEY", "DEEPL_API_KEY"],
                    &[],
                    false,
                ),
                DEEPL_SERVICES,
                LLM_PURPOSES,
                DEEPL_TRANSLATION_LANGUAGES,
                false,
            ),
            MICROSOFT_PROVIDER => (
                "Microsoft Translator",
                ProviderCategory::CloudProvider,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Regional,
                    &["VRCS_MICROSOFT_TRANSLATOR_KEY"],
                    &[],
                    false,
                ),
                MICROSOFT_SERVICES,
                LLM_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                false,
            ),
            LM_STUDIO_PROVIDER => (
                "LM Studio",
                ProviderCategory::LocalService,
                connection(
                    ApiAuthMode::None,
                    BaseUrlPolicy::Editable("http://127.0.0.1:1234/v1"),
                    &[],
                    &["VRCS_OPENAI_COMPATIBLE_API_KEY"],
                    true,
                ),
                LM_STUDIO_SERVICES,
                LLM_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            OLLAMA_PROVIDER => (
                "Ollama",
                ProviderCategory::LocalService,
                connection(
                    ApiAuthMode::None,
                    BaseUrlPolicy::Editable("http://127.0.0.1:11434/v1"),
                    &[],
                    &["VRCS_OPENAI_COMPATIBLE_API_KEY"],
                    true,
                ),
                OLLAMA_SERVICES,
                LLM_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            OPENAI_COMPATIBLE_PROVIDER => (
                "Custom OpenAI Compatible",
                ProviderCategory::CustomProtocol,
                connection(
                    ApiAuthMode::Bearer,
                    BaseUrlPolicy::Editable(""),
                    &["VRCS_OPENAI_COMPATIBLE_API_KEY"],
                    &[],
                    true,
                ),
                CUSTOM_SERVICES,
                LLM_PURPOSES,
                LLM_TRANSLATION_LANGUAGES,
                true,
            ),
            _ => return None,
        };
    let support_levels = service_support_levels(services);
    let capabilities = capabilities_from_services(
        services,
        connection.auth_mode == ApiAuthMode::Bearer,
        category == ProviderCategory::LocalService,
        languages,
        custom_languages,
    );
    Some(ProviderDefinition {
        id: provider_id(provider),
        display_name,
        category,
        connection,
        services,
        purposes,
        support_levels,
        capabilities,
        presets: if provider == OPENAI_COMPATIBLE_PROVIDER {
            OPENAI_COMPATIBLE_PRESETS
        } else {
            &[]
        },
    })
}

const fn connection(
    auth_mode: ApiAuthMode,
    base_url: BaseUrlPolicy,
    environment_variables: &'static [&'static str],
    legacy_environment_variables: &'static [&'static str],
    allow_custom_headers: bool,
) -> ProviderConnectionDefinition {
    ProviderConnectionDefinition {
        auth_mode,
        base_url,
        environment_variables,
        legacy_environment_variables,
        allow_custom_headers,
    }
}

fn provider_id(provider: &str) -> &'static str {
    match provider {
        ALIBABA_PROVIDER => ALIBABA_PROVIDER,
        ALIBABA_TOKEN_PLAN_PROVIDER => ALIBABA_TOKEN_PLAN_PROVIDER,
        OPENAI_PROVIDER => OPENAI_PROVIDER,
        OPENAI_COMPATIBLE_PROVIDER => OPENAI_COMPATIBLE_PROVIDER,
        GEMINI_PROVIDER => GEMINI_PROVIDER,
        DEEPL_PROVIDER => DEEPL_PROVIDER,
        MICROSOFT_PROVIDER => MICROSOFT_PROVIDER,
        GROQ_PROVIDER => GROQ_PROVIDER,
        OPENROUTER_PROVIDER => OPENROUTER_PROVIDER,
        DEEPSEEK_PROVIDER => DEEPSEEK_PROVIDER,
        LM_STUDIO_PROVIDER => LM_STUDIO_PROVIDER,
        OLLAMA_PROVIDER => OLLAMA_PROVIDER,
        _ => unreachable!("provider was matched before conversion"),
    }
}

fn service_support_levels(services: &[ProviderServiceDefinition]) -> CapabilitySupportLevels {
    CapabilitySupportLevels {
        asr: best_support(services.iter().filter_map(|service| service.asr_support)),
        translation: best_support(
            services
                .iter()
                .filter_map(|service| service.translation_support),
        ),
    }
}

fn best_support(levels: impl Iterator<Item = SupportLevel>) -> Option<SupportLevel> {
    levels.reduce(|current, next| {
        if current == SupportLevel::Native || next == SupportLevel::Native {
            SupportLevel::Native
        } else {
            SupportLevel::ProtocolCompatible
        }
    })
}

fn capabilities_from_services(
    services: &[ProviderServiceDefinition],
    requires_api_key: bool,
    is_local: bool,
    supported_languages: &'static [&'static str],
    supports_custom_translation_language: bool,
) -> ProviderCapabilities {
    ProviderCapabilities {
        supports_streaming: services.iter().any(|service| service.supports_streaming),
        supports_model_listing: services
            .iter()
            .any(|service| service.supports_model_listing),
        requires_api_key,
        is_local,
        supports_context: services.iter().any(|service| service.supports_context),
        supports_text_generation: services
            .iter()
            .any(|service| service.capabilities.contains(&CAPABILITY_TEXT_GENERATION)),
        supports_translation: services
            .iter()
            .any(|service| service.capabilities.contains(&CAPABILITY_TEXT_TRANSLATION)),
        supports_asr: services
            .iter()
            .any(|service| service.capabilities.contains(&CAPABILITY_SPEECH_TO_TEXT)),
        supports_custom_translation_language,
        supported_languages,
    }
}

#[cfg(test)]
fn service(provider: &str, service_id: &str) -> Option<&'static ProviderServiceDefinition> {
    definition(provider)?
        .services
        .iter()
        .find(|service| service.id == service_id)
}

pub fn recognition_service(
    service_id: &str,
) -> Option<(&'static str, &'static ProviderServiceDefinition)> {
    catalog().into_iter().find_map(|provider| {
        provider
            .services
            .iter()
            .find(|service| {
                service.id == service_id
                    && service.capabilities.contains(&CAPABILITY_SPEECH_TO_TEXT)
            })
            .map(|service| (provider.id, service))
    })
}

/// Validates a user-configured recognition model name.
///
/// Provider model lists are suggestions for the UI, not an allowlist. The upstream service
/// remains responsible for deciding whether a well-formed model name exists and is usable.
pub fn recognition_model_supported(_service: &ProviderServiceDefinition, model: &str) -> bool {
    let trimmed = model.trim();
    model == trimmed && !model.is_empty() && model.chars().count() <= 200
}

fn recognition_catalog_model_supported(service: &ProviderServiceDefinition, model: &str) -> bool {
    if !recognition_model_supported(service, model) {
        return false;
    }
    match service.adapter {
        ServiceAdapter::QwenRealtime => versioned_model(model, "qwen3-asr-flash-realtime"),
        ServiceAdapter::AlibabaTokenPlanRealtime => service.models.contains(&model),
        ServiceAdapter::FunAsrRealtime => {
            versioned_model(model, "qwen-audio-3.0-asr-flash-streaming")
                || versioned_model(model, "fun-asr-realtime")
        }
        _ => service.models.contains(&model),
    }
}

pub fn compatible_service_models(
    service: &ProviderServiceDefinition,
    models: Vec<String>,
) -> Vec<String> {
    models
        .into_iter()
        .filter(|model| recognition_catalog_model_supported(service, model))
        .collect()
}

fn versioned_model(model: &str, base: &str) -> bool {
    if model == base {
        return true;
    }
    let Some(version) = model
        .strip_prefix(base)
        .and_then(|suffix| suffix.strip_prefix('-'))
    else {
        return false;
    };
    let bytes = version.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedService {
    pub provider: ProviderDefinition,
    pub service: &'static ProviderServiceDefinition,
}

pub fn resolve_profile_service(
    profile: &ApiProfile,
    service_id: &str,
) -> Result<ResolvedService, String> {
    let provider = definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    let service = provider
        .services
        .iter()
        .find(|service| service.id == service_id)
        .ok_or_else(|| {
            format!(
                "Service {service_id} does not belong to provider {}",
                profile.provider
            )
        })?;
    if !service.capabilities.iter().any(|capability| {
        profile
            .enabled_capabilities
            .iter()
            .any(|enabled| enabled == capability)
    }) {
        return Err(format!(
            "API profile {} has not enabled service {service_id}",
            profile.id
        ));
    }
    Ok(ResolvedService { provider, service })
}

pub fn resolve_profile_capability(
    profile: &ApiProfile,
    capability: &str,
) -> Result<ResolvedService, String> {
    if !profile
        .enabled_capabilities
        .iter()
        .any(|enabled| enabled == capability)
    {
        return Err(format!(
            "API profile {} has not enabled capability {capability}",
            profile.id
        ));
    }
    let provider = definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    let service = provider
        .services
        .iter()
        .find(|service| service.capabilities.contains(&capability))
        .ok_or_else(|| {
            format!(
                "Provider {} does not support capability {capability}",
                profile.provider
            )
        })?;
    Ok(ResolvedService { provider, service })
}

pub fn effective_base_url(profile: &ApiProfile) -> Result<String, String> {
    let provider = definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    match provider.connection.base_url {
        BaseUrlPolicy::Fixed(value) => Ok(value.into()),
        BaseUrlPolicy::Regional => Err(format!(
            "Provider {} uses a region-specific endpoint",
            profile.provider
        )),
        BaseUrlPolicy::Editable(default) => profile
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or((!default.is_empty()).then_some(default))
            .map(str::to_owned)
            .ok_or_else(|| format!("Provider {} requires a Base URL", profile.provider)),
    }
}

pub fn provider_capability_ids(provider: &str) -> Option<Vec<&'static str>> {
    let provider = definition(provider)?;
    let mut capabilities = Vec::new();
    for service in provider.services {
        for capability in service.capabilities {
            if !capabilities.contains(capability) {
                capabilities.push(*capability);
            }
        }
    }
    Some(capabilities)
}

pub fn profile_capabilities(profile: &ApiProfile) -> Option<ProviderCapabilities> {
    let provider = definition(&profile.provider)?;
    let enabled_services = provider.services.iter().filter(|service| {
        service.capabilities.iter().any(|capability| {
            profile
                .enabled_capabilities
                .iter()
                .any(|enabled| enabled == capability)
        })
    });
    let enabled_services = enabled_services.copied().collect::<Vec<_>>();
    let mut capabilities = capabilities_from_services(
        &enabled_services,
        profile.requires_api_key(),
        profile.is_local || provider.category == ProviderCategory::LocalService,
        provider.capabilities.supported_languages,
        provider.capabilities.supports_custom_translation_language,
    );
    capabilities.supports_text_generation &= profile
        .enabled_capabilities
        .iter()
        .any(|value| value == CAPABILITY_TEXT_GENERATION);
    capabilities.supports_translation &= profile
        .enabled_capabilities
        .iter()
        .any(|value| value == CAPABILITY_TEXT_TRANSLATION);
    capabilities.supports_asr &= profile
        .enabled_capabilities
        .iter()
        .any(|value| value == CAPABILITY_SPEECH_TO_TEXT);
    Some(capabilities)
}

pub fn profile_support_levels(profile: &ApiProfile) -> Option<CapabilitySupportLevels> {
    let provider = definition(&profile.provider)?;
    let services = provider.services.iter().filter(|service| {
        service.capabilities.iter().any(|capability| {
            profile
                .enabled_capabilities
                .iter()
                .any(|enabled| enabled == capability)
        })
    });
    let services = services.copied().collect::<Vec<_>>();
    let mut levels = service_support_levels(&services);
    if !profile
        .enabled_capabilities
        .iter()
        .any(|capability| capability == CAPABILITY_SPEECH_TO_TEXT)
    {
        levels.asr = None;
    }
    if !profile
        .enabled_capabilities
        .iter()
        .any(|capability| capability == CAPABILITY_TEXT_TRANSLATION)
    {
        levels.translation = None;
    }
    Some(levels)
}

#[cfg(test)]
pub fn effective_purpose(profile: &ApiProfile) -> &'static str {
    let asr = profile
        .enabled_capabilities
        .iter()
        .any(|value| value == CAPABILITY_SPEECH_TO_TEXT);
    let text = profile.enabled_capabilities.iter().any(|value| {
        matches!(
            value.as_str(),
            CAPABILITY_TEXT_GENERATION | CAPABILITY_TEXT_TRANSLATION
        )
    });
    match (asr, text) {
        (true, true) => API_PURPOSE_SHARED,
        (true, false) => API_PURPOSE_ASR,
        _ => API_PURPOSE_LLM,
    }
}

#[cfg(test)]
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
    [CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION]
        .into_iter()
        .find_map(|capability| resolve_profile_capability(profile, capability).ok())
        .is_some_and(|resolved| resolved.service.supports_model_listing)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(provider: &str, capabilities: &[&str]) -> ApiProfile {
        ApiProfile {
            id: "profile".into(),
            name: "Profile".into(),
            provider: provider.into(),
            enabled_capabilities: capabilities.iter().map(|value| (*value).into()).collect(),
            ..ApiProfile::default()
        }
    }

    #[test]
    fn catalog_separates_brands_protocols_and_services() {
        let groq = definition(GROQ_PROVIDER).unwrap();
        assert_eq!(groq.category, ProviderCategory::CloudProvider);
        let transcription = service(GROQ_PROVIDER, SERVICE_GROQ_TRANSCRIPTION).unwrap();
        assert_eq!(
            transcription.recognition_transport,
            Some(RecognitionTransport::SegmentedUpload)
        );
        assert!(!transcription.partial_results);
        assert_eq!(transcription.models, GROQ_ASR_MODELS);

        let custom = definition(OPENAI_COMPATIBLE_PROVIDER).unwrap();
        assert_eq!(custom.category, ProviderCategory::CustomProtocol);
        assert_eq!(custom.presets.len(), 1);
        assert_eq!(custom.presets[0].id, "custom");
    }

    #[test]
    fn gemini_catalog_exposes_transcription_under_the_existing_provider() {
        let gemini = definition(GEMINI_PROVIDER).unwrap();
        assert_eq!(gemini.display_name, "Gemini");
        assert_eq!(gemini.services.len(), 2);
        assert_eq!(gemini.purposes, SHARED_PURPOSES);

        let transcription = service(GEMINI_PROVIDER, SERVICE_GEMINI_TRANSCRIBE).unwrap();
        assert_eq!(transcription.display_name, "Gemini Transcribe");
        assert_eq!(
            transcription.recognition_transport,
            Some(RecognitionTransport::RealtimeStream)
        );
        assert_eq!(transcription.models, GEMINI_ASR_MODELS);
        assert!(transcription.partial_results);
    }

    #[test]
    fn enabled_capabilities_limit_derived_compatibility_fields() {
        let capabilities =
            profile_capabilities(&profile(ALIBABA_PROVIDER, &[CAPABILITY_SPEECH_TO_TEXT])).unwrap();
        assert!(capabilities.supports_asr);
        assert!(!capabilities.supports_translation);
        assert!(!capabilities.supports_text_generation);
        assert!(capabilities.supports_model_listing);
    }

    #[test]
    fn alibaba_recognition_catalog_accepts_supported_realtime_families() {
        let qwen = service(ALIBABA_PROVIDER, SERVICE_QWEN_REALTIME).unwrap();
        let fun_asr = service(ALIBABA_PROVIDER, SERVICE_FUN_ASR_REALTIME).unwrap();

        assert!(qwen.supports_model_listing);
        assert!(recognition_catalog_model_supported(
            qwen,
            "qwen3-asr-flash-realtime-2026-02-10"
        ));
        assert!(recognition_catalog_model_supported(
            fun_asr,
            "fun-asr-realtime-2025-11-07"
        ));
        assert!(recognition_catalog_model_supported(
            fun_asr,
            "qwen-audio-3.0-asr-flash-streaming"
        ));
        assert!(!recognition_catalog_model_supported(
            qwen,
            "qwen3-asr-flash"
        ));
        assert!(!recognition_catalog_model_supported(
            qwen,
            "qwen-audio-3.0-asr-flash-streaming"
        ));

        assert_eq!(
            compatible_service_models(
                qwen,
                vec![
                    "qwen3-asr-flash".into(),
                    "qwen3-asr-flash-realtime-2026-02-10".into(),
                ],
            ),
            ["qwen3-asr-flash-realtime-2026-02-10"]
        );
        assert_eq!(
            compatible_service_models(
                fun_asr,
                vec![
                    "qwen-audio-3.0-asr-flash-streaming".into(),
                    "qwen3-asr-flash".into(),
                ],
            ),
            ["qwen-audio-3.0-asr-flash-streaming"]
        );
    }

    #[test]
    fn recognition_model_validation_accepts_custom_names() {
        let openai = service(OPENAI_PROVIDER, SERVICE_OPENAI_REALTIME).unwrap();

        assert!(recognition_model_supported(openai, "custom-transcribe-v1"));
        assert!(!recognition_model_supported(openai, ""));
        assert!(!recognition_model_supported(
            openai,
            " custom-transcribe-v1"
        ));
        assert!(!recognition_model_supported(openai, &"m".repeat(201)));
    }

    #[test]
    fn token_plan_catalog_exposes_text_and_realtime_asr_services() {
        let provider = definition("alibaba_token_plan").unwrap();
        assert_eq!(provider.category, ProviderCategory::CloudProvider);
        assert_eq!(
            provider.connection.base_url,
            BaseUrlPolicy::Fixed(
                "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
            )
        );

        assert_eq!(provider.services.len(), 2);
        assert!(provider.services[0]
            .capabilities
            .contains(&CAPABILITY_TEXT_GENERATION));
        let realtime = provider
            .services
            .iter()
            .find(|service| service.id == SERVICE_TOKEN_PLAN_REALTIME)
            .unwrap();
        assert_eq!(
            realtime.recognition_transport,
            Some(RecognitionTransport::RealtimeStream)
        );
        assert_eq!(realtime.models, ["qwen-audio-3.0-realtime-plus"]);
        assert_eq!(
            compatible_service_models(
                realtime,
                vec![
                    "qwen-audio-3.0-asr-flash".into(),
                    "qwen-audio-3.0-realtime-plus".into(),
                ],
            ),
            ["qwen-audio-3.0-realtime-plus"]
        );
    }

    #[test]
    fn resolves_adapters_without_using_legacy_presets() {
        let mut deepseek = profile(
            DEEPSEEK_PROVIDER,
            &[CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION],
        );
        deepseek.preset_id = Some("ignored-legacy-value".into());
        let resolved = resolve_profile_capability(&deepseek, CAPABILITY_TEXT_GENERATION).unwrap();
        assert_eq!(
            resolved.service.adapter,
            ServiceAdapter::OpenAiChatCompletions {
                behavior: OpenAiProtocolBehavior::DeepSeek
            }
        );
    }

    #[test]
    fn translation_language_capabilities_are_provider_specific() {
        let llm = profile(
            OPENAI_PROVIDER,
            &[CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION],
        );
        let deepl = profile(DEEPL_PROVIDER, &[CAPABILITY_TEXT_TRANSLATION]);

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
