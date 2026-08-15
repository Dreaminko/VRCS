use super::*;
use crate::providers::{
    self, ALIBABA_PROVIDER, API_PURPOSE_ASR, API_PURPOSE_LLM, API_PURPOSE_SHARED, DEEPL_PROVIDER,
    GEMINI_PROVIDER, OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER,
};

#[test]
fn rejects_zero_osc_port() {
    let mut config = AppConfig::default();
    config.osc.port = 0;
    assert!(config.validate_settings().is_err());
}

#[test]
fn qwen_is_the_default_backend() {
    let config = AppConfig::default();
    assert_eq!(config.asr.backend, "qwen_realtime");
    assert!(config.asr.api_profiles.is_empty());
    assert!(config.validate_settings().is_ok());
}

#[test]
fn disabled_output_mode_cannot_select_a_device() {
    let mut config = AppConfig::default();
    config.audio.output.mode = "disabled".into();
    assert!(config.validate_settings().is_ok());

    config.audio.output.device_id = Some(3);
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "VRChat or disabled output mode cannot specify a system output device"
    );
}

#[test]
fn microphone_trigger_threshold_is_bounded() {
    let mut config = AppConfig::default();
    config.audio.microphone.trigger_threshold_dbfs = -80.0;
    assert!(config.validate_settings().is_ok());

    config.audio.microphone.trigger_threshold_dbfs = -10.0;
    assert!(config.validate_settings().is_ok());

    config.audio.microphone.trigger_threshold_dbfs = -81.0;
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Microphone trigger_threshold_dbfs must be between -80 and -10"
    );
}

#[test]
fn translation_accepts_direct_and_llm_profiles() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "deepl-one".into(),
        name: "DeepL".into(),
        provider: DEEPL_PROVIDER.into(),
        region: None,
        workspace_id: None,
        base_url: None,
        purpose: None,
        ..ApiProfile::default()
    });
    config.translation.mode = "manual".into();
    config.translation.profile_id = Some("deepl-one".into());
    assert!(config.validate_settings().is_ok());

    config.asr.api_profiles.push(ApiProfile {
        id: "openai-one".into(),
        name: "OpenAI".into(),
        provider: OPENAI_PROVIDER.into(),
        region: None,
        workspace_id: None,
        base_url: None,
        purpose: None,
        ..ApiProfile::default()
    });
    config.translation.profile_id = Some("openai-one".into());
    config.translation.model.clear();
    assert!(config.validate_settings().is_err());
}

#[test]
fn translation_languages_follow_the_selected_provider() {
    let mut config = AppConfig::default();
    config.asr.api_profiles = vec![
        ApiProfile {
            id: "deepl-one".into(),
            name: "DeepL".into(),
            provider: DEEPL_PROVIDER.into(),
            ..ApiProfile::default()
        },
        ApiProfile {
            id: "openai-one".into(),
            name: "OpenAI".into(),
            provider: OPENAI_PROVIDER.into(),
            ..ApiProfile::default()
        },
    ];
    config.translation.mode = "manual".into();
    config.translation.target_language = "hi".into();
    config.translation.profile_id = Some("deepl-one".into());
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The selected API profile does not support target language: hi"
    );

    config.translation.profile_id = Some("openai-one".into());
    config.translation.target_language = "tlh-Latn".into();
    assert!(config.validate_settings().is_ok());

    config.translation.target_language = "not a language".into();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Invalid translation target language: not a language"
    );
}

#[test]
fn gemini_profiles_are_llm_only_and_require_a_model() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "gemini-one".into(),
        name: "Gemini".into(),
        provider: GEMINI_PROVIDER.into(),
        region: None,
        workspace_id: None,
        base_url: None,
        purpose: Some(API_PURPOSE_LLM.into()),
        ..ApiProfile::default()
    });
    config.translation.mode = "manual".into();
    config.translation.profile_id = Some("gemini-one".into());
    config.translation.model = "gemini-2.5-flash".into();
    assert!(config.validate_settings().is_ok());

    config.translation.model.clear();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The LLM translation model cannot be empty"
    );
    config.translation.model = "gemini-2.5-flash".into();
    config.asr.api_profiles[0].purpose = Some(API_PURPOSE_SHARED.into());
    assert!(config.validate_settings().is_err());
}

#[test]
fn api_profile_purposes_separate_asr_and_translation() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "openai-asr".into(),
        name: "OpenAI ASR".into(),
        provider: OPENAI_PROVIDER.into(),
        region: None,
        workspace_id: None,
        base_url: None,
        purpose: Some(API_PURPOSE_ASR.into()),
        ..ApiProfile::default()
    });
    config.asr.active_api_profiles.openai = Some("openai-asr".into());
    assert!(config.validate_settings().is_ok());

    config.translation.mode = "manual".into();
    config.translation.profile_id = Some("openai-asr".into());
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The selected API profile does not support translation"
    );

    config.translation.mode = "disabled".into();
    config.translation.profile_id = None;
    config.asr.api_profiles[0].purpose = Some(API_PURPOSE_LLM.into());
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The active API profile does not support realtime speech recognition"
    );
}

#[test]
fn legacy_api_profile_purposes_are_inferred() {
    let official = ApiProfile {
        id: "openai".into(),
        name: "OpenAI".into(),
        provider: OPENAI_PROVIDER.into(),
        region: None,
        workspace_id: None,
        base_url: None,
        purpose: None,
        ..ApiProfile::default()
    };
    let compatible = ApiProfile {
        provider: OPENAI_COMPATIBLE_PROVIDER.into(),
        base_url: Some("https://api.deepseek.com/v1".into()),
        ..official.clone()
    };
    assert_eq!(providers::effective_purpose(&official), API_PURPOSE_SHARED);
    assert_eq!(providers::effective_purpose(&compatible), API_PURPOSE_LLM);
}

#[test]
fn microphone_translation_requires_a_profile_when_enabled() {
    let mut config = AppConfig::default();
    config.translation.translate_microphone = true;
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "A translation API profile must be selected"
    );
}

#[test]
fn validates_openai_compatible_profiles_and_keeps_them_out_of_realtime_asr() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "deepseek".into(),
        name: "DeepSeek".into(),
        provider: OPENAI_COMPATIBLE_PROVIDER.into(),
        region: None,
        workspace_id: None,
        base_url: Some("https://api.deepseek.com/v1".into()),
        purpose: None,
        ..ApiProfile::default()
    });
    config.translation.mode = "manual".into();
    config.translation.profile_id = Some("deepseek".into());
    config.translation.model = "deepseek-chat".into();
    assert!(config.validate_settings().is_ok());

    config.asr.active_api_profiles.openai = Some("deepseek".into());
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The active API profile does not match provider openai"
    );

    config.asr.active_api_profiles.openai = None;
    config.asr.api_profiles[0].base_url = Some("https://api.deepseek.com/v1?token=secret".into());
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The OpenAI-compatible Base URL cannot contain credentials, a query, or a fragment"
    );
}

#[test]
fn validates_compatible_timeout_and_safe_headers() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "local".into(),
        name: "Local".into(),
        provider: OPENAI_COMPATIBLE_PROVIDER.into(),
        base_url: Some("http://127.0.0.1:11434/v1".into()),
        purpose: Some(API_PURPOSE_LLM.into()),
        preset_id: Some("ollama".into()),
        auth_mode: ApiAuthMode::None,
        is_local: true,
        headers: vec![HttpHeaderConfig {
            name: "X-Client-Name".into(),
            value: "VRCS".into(),
        }],
        ..ApiProfile::default()
    });
    assert!(config.validate_settings().is_ok());

    config.asr.api_profiles[0].headers[0].name = "Authorization".into();
    assert!(config.validate_settings().is_err());
    config.asr.api_profiles[0].headers.clear();
    config.asr.api_profiles[0].timeout_ms = 999;
    assert!(config.validate_settings().is_err());
}

#[test]
fn bearer_credentials_require_https_outside_loopback() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "remote-http".into(),
        name: "Remote HTTP".into(),
        provider: OPENAI_COMPATIBLE_PROVIDER.into(),
        base_url: Some("http://192.0.2.1/v1".into()),
        purpose: Some(API_PURPOSE_LLM.into()),
        auth_mode: ApiAuthMode::Bearer,
        ..ApiProfile::default()
    });

    assert_eq!(
        config.validate_settings().unwrap_err(),
        "OpenAI-compatible profiles cannot send Bearer credentials over remote HTTP"
    );
    config.asr.api_profiles[0].base_url = Some("http://localhost:1234/v1".into());
    assert!(config.validate_settings().is_ok());
    config.asr.api_profiles[0].base_url = Some("http://192.0.2.1/v1".into());
    config.asr.api_profiles[0].auth_mode = ApiAuthMode::None;
    assert!(config.validate_settings().is_ok());
}

#[test]
fn external_api_requires_token_authentication_outside_loopback() {
    let mut config = AppConfig::default();
    config.external_api.host = "0.0.0.0".into();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "External API token authentication is required outside loopback"
    );
    config.external_api.require_token = true;
    assert!(config.validate_settings().is_ok());
}

#[test]
fn validates_translation_prompt_variables_and_limits() {
    let mut prompt = TranslationPromptConfig::default();
    assert!(validate_translation_prompt(&prompt).is_ok());

    prompt.system_prompt = "Translate {unknown}".into();
    assert_eq!(
        validate_translation_prompt(&prompt).unwrap_err(),
        "Unsupported translation prompt variable: {unknown}"
    );
    prompt.system_prompt = DEFAULT_TRANSLATION_SYSTEM_PROMPT.into();
    prompt.max_messages = 51;
    assert_eq!(
        validate_translation_prompt(&prompt).unwrap_err(),
        "Translation context max_messages must be between 1 and 50"
    );
    prompt.max_messages = 5;
    prompt.max_chars = 199;
    assert_eq!(
        validate_translation_prompt(&prompt).unwrap_err(),
        "Translation context max_chars must be between 200 and 12000"
    );
    prompt.max_chars = 4_000;
    prompt.glossary.push(GlossaryEntry {
        source: "术".repeat(201),
        target: None,
        category: GlossaryCategory::Custom,
        case_sensitive: false,
    });
    assert_eq!(
        validate_translation_prompt(&prompt).unwrap_err(),
        "Translation glossary source must contain 1 to 200 single-line characters"
    );
}

#[test]
fn validates_glossary_sources_and_subscription_urls() {
    let entry = |source: &str, case_sensitive| GlossaryEntry {
        source: source.into(),
        target: None,
        category: GlossaryCategory::Custom,
        case_sensitive,
    };
    let mut prompt = TranslationPromptConfig::default();
    prompt.glossary_sources = vec![
        GlossarySource::Local {
            id: "local".into(),
            name: "Local glossary".into(),
            enabled: true,
            entries: vec![entry("VRChat", false)],
        },
        GlossarySource::Subscription {
            id: "remote".into(),
            url: "https://example.com/glossary.json".into(),
            display_name: None,
            enabled: true,
        },
    ];
    assert!(validate_translation_prompt(&prompt).is_ok());

    if let GlossarySource::Subscription { url, .. } = &mut prompt.glossary_sources[1] {
        *url = "http://127.0.0.1:8080/glossary.json".into();
    }
    assert!(validate_translation_prompt(&prompt).is_ok());
    if let GlossarySource::Subscription { url, .. } = &mut prompt.glossary_sources[1] {
        *url = "http://example.com/glossary.json".into();
    }
    assert_eq!(
        validate_translation_prompt(&prompt).unwrap_err(),
        "Glossary source URL must use HTTPS, except for loopback HTTP addresses"
    );

    if let GlossarySource::Subscription { id, url, .. } = &mut prompt.glossary_sources[1] {
        *id = " local ".into();
        *url = "https://example.com/glossary.json".into();
    }
    assert_eq!(
        validate_translation_prompt(&prompt).unwrap_err(),
        "Glossary source id must be unique: local"
    );

    if let GlossarySource::Local { entries, .. } = &mut prompt.glossary_sources[0] {
        entries.push(entry("vrchat", false));
    }
    prompt.glossary_sources.pop();
    assert_eq!(
        validate_translation_prompt(&prompt).unwrap_err(),
        "Local glossary contains a duplicate source term: vrchat"
    );

    if let GlossarySource::Local { entries, .. } = &mut prompt.glossary_sources[0] {
        entries[1].case_sensitive = true;
    }
    assert!(validate_translation_prompt(&prompt).is_ok());
    if let GlossarySource::Local { entries, .. } = &mut prompt.glossary_sources[0] {
        entries[1].source = "line\nbreak".into();
    }
    assert_eq!(
        validate_translation_prompt(&prompt).unwrap_err(),
        "Local glossary source must contain 1 to 200 single-line characters"
    );
}

#[test]
fn api_profiles_require_unique_names_and_matching_active_provider() {
    let mut config = AppConfig::default();
    config.asr.api_profiles = vec![
        ApiProfile {
            id: "alibaba-one".into(),
            name: "Personal".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("china_beijing".into()),
            workspace_id: Some("workspace-one".into()),
            base_url: None,
            purpose: None,
            ..ApiProfile::default()
        },
        ApiProfile {
            id: "alibaba-two".into(),
            name: "personal".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("singapore".into()),
            workspace_id: Some("workspace-two".into()),
            base_url: None,
            purpose: None,
            ..ApiProfile::default()
        },
    ];
    assert!(config.validate_settings().is_err());

    config.asr.api_profiles[1].name = "Work".into();
    config.asr.active_api_profiles.openai = Some("alibaba-two".into());
    assert!(config.validate_settings().is_err());

    config.asr.active_api_profiles.openai = None;
    config.asr.active_api_profiles.alibaba_cloud = Some("alibaba-two".into());
    assert!(config.validate_settings().is_ok());
}

#[test]
fn validates_fun_asr_specific_limits() {
    let mut config = AppConfig::default();
    config.asr.backend = "fun_asr_realtime".into();
    config.asr.fun_asr.context = "字".repeat(400);
    assert!(config.validate_settings().is_ok());

    config.asr.fun_asr.context.push('字');
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Fun-ASR context cannot exceed 400 characters"
    );
}

#[test]
fn anki_name_limits_count_unicode_characters() {
    let mut config = AppConfig::default();
    config.anki.deck = "学".repeat(100);
    assert!(config.validate_settings().is_ok());
    config.anki.deck.push('ぶ');
    assert!(config.validate_settings().is_err());
}
