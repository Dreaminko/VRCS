use super::validation::validate_glossary;
use super::*;
use crate::providers::{
    self, ALIBABA_PROVIDER, CAPABILITY_SPEECH_TO_TEXT, CAPABILITY_TEXT_GENERATION,
    CAPABILITY_TEXT_TRANSLATION, DEEPL_PROVIDER, GEMINI_PROVIDER, GROQ_PROVIDER, OLLAMA_PROVIDER,
    OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER, SERVICE_FUN_ASR_REALTIME,
    SERVICE_GEMINI_TRANSCRIBE, SERVICE_GROQ_TRANSCRIPTION, SERVICE_OPENAI_REALTIME,
    SERVICE_QWEN_REALTIME,
};

#[test]
fn rejects_zero_osc_port() {
    let mut config = AppConfig::default();
    config.osc.port = 0;
    assert!(config.validate_settings().is_err());
}

#[test]
fn rejects_zero_vrcx_port() {
    let mut config = AppConfig::default();
    config.vrcx.port = 0;
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "VRCX-0 Integration API port must be between 1 and 65535"
    );
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
fn output_trigger_threshold_is_bounded() {
    let mut config = AppConfig::default();
    config.audio.output.trigger_threshold_dbfs = -80.0;
    assert!(config.validate_settings().is_ok());

    config.audio.output.trigger_threshold_dbfs = -10.0;
    assert!(config.validate_settings().is_ok());

    config.audio.output.trigger_threshold_dbfs = -81.0;
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Output trigger_threshold_dbfs must be between -80 and -10"
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
fn validates_vr_overlay_boundaries_and_cross_fields() {
    let mut config = AppConfig::default();
    config.vr_overlay.headset.offset_x_m = -2.0;
    config.vr_overlay.headset.offset_y_m = 2.0;
    config.vr_overlay.headset.distance_m = 0.25;
    config.vr_overlay.headset.pitch_deg = 90.0;
    config.vr_overlay.headset.yaw_deg = -180.0;
    config.vr_overlay.headset.roll_deg = 180.0;
    config.vr_overlay.headset.width_m = 3.0;
    config.vr_overlay.headset.opacity = 0.10;
    config.vr_overlay.headset.display_seconds = 5.0;
    config.vr_overlay.headset.fade_seconds = 5.0;
    config.vr_overlay.headset.font_size_px = 96;
    config.vr_overlay.headset.background_opacity = 0.0;
    config.vr_overlay.wrist.max_entries = 10;
    config.vr_overlay.wrist.idle_hide_seconds = 120;
    config.vr_overlay.wrist.offset_x_m = -0.5;
    config.vr_overlay.wrist.offset_y_m = 0.5;
    config.vr_overlay.wrist.offset_z_m = -0.5;
    config.vr_overlay.wrist.pitch_deg = -180.0;
    config.vr_overlay.wrist.yaw_deg = 180.0;
    config.vr_overlay.wrist.roll_deg = -180.0;
    config.vr_overlay.wrist.width_m = 1.0;
    config.vr_overlay.wrist.opacity = 1.0;
    config.vr_overlay.wrist.font_size_px = 72;
    config.vr_overlay.wrist.background_opacity = 1.0;
    assert!(config.validate_settings().is_ok());

    config.vr_overlay.headset.fade_seconds = 5.1;
    assert!(config.validate_settings().is_err());
    config.vr_overlay.headset.fade_seconds = 1.0;
    config.vr_overlay.wrist.max_entries = 2;
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "VR Overlay wrist max_entries must be between 3 and 10"
    );
    config.vr_overlay.wrist.max_entries = 5;
    config.vr_overlay.wrist.idle_hide_seconds = 4;
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "VR Overlay wrist idle_hide_seconds must be 0 or between 5 and 120"
    );
}

#[test]
fn rejects_invalid_vr_overlay_enums_and_non_finite_values() {
    let mut config = AppConfig::default();
    config.vr_overlay.translation_display = "unknown".into();
    assert!(config.validate_settings().is_err());
    config.vr_overlay.translation_display = "all_languages".into();
    config.vr_overlay.headset.content_mode = "unknown".into();
    assert!(config.validate_settings().is_err());
    config.vr_overlay.headset.content_mode = "bilingual".into();
    config.vr_overlay.wrist.hand = "either".into();
    assert!(config.validate_settings().is_err());
    config.vr_overlay.wrist.hand = "dominant".into();
    config.vr_overlay.wrist.dominant_hand = "either".into();
    assert!(config.validate_settings().is_err());
    config.vr_overlay.wrist.dominant_hand = "right".into();
    config.vr_overlay.headset.opacity = f32::NAN;
    assert!(config.validate_settings().is_err());
}

fn text_capabilities() -> Vec<String> {
    vec![
        CAPABILITY_TEXT_GENERATION.into(),
        CAPABILITY_TEXT_TRANSLATION.into(),
    ]
}

fn set_translation_profile(config: &mut AppConfig, profile_id: Option<&str>) {
    for target in config
        .translation
        .speaker_targets
        .iter_mut()
        .chain(&mut config.translation.microphone_targets)
    {
        target.profile_id = profile_id.map(str::to_owned);
    }
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
        enabled_capabilities: vec![CAPABILITY_TEXT_TRANSLATION.into()],
        ..ApiProfile::default()
    });
    config.translation.mode = "manual".into();
    set_translation_profile(&mut config, Some("deepl-one"));
    assert!(config.validate_settings().is_ok());

    config.asr.api_profiles.push(ApiProfile {
        id: "openai-one".into(),
        name: "OpenAI".into(),
        provider: OPENAI_PROVIDER.into(),
        region: None,
        workspace_id: None,
        base_url: None,
        enabled_capabilities: text_capabilities(),
        ..ApiProfile::default()
    });
    set_translation_profile(&mut config, Some("openai-one"));
    config.translation.speaker_targets[0].model.clear();
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
            enabled_capabilities: vec![CAPABILITY_TEXT_TRANSLATION.into()],
            ..ApiProfile::default()
        },
        ApiProfile {
            id: "openai-one".into(),
            name: "OpenAI".into(),
            provider: OPENAI_PROVIDER.into(),
            enabled_capabilities: text_capabilities(),
            ..ApiProfile::default()
        },
    ];
    config.translation.mode = "manual".into();
    config.translation.speaker_targets[0].target_language = "hi".into();
    set_translation_profile(&mut config, Some("deepl-one"));
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The selected API profile does not support target language: hi"
    );

    config.translation.speaker_targets[0].target_language = "ja".into();
    config.translation.microphone_targets[0].target_language = "hi".into();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The selected API profile does not support target language: hi"
    );

    config.translation.mode = "manual".into();
    set_translation_profile(&mut config, Some("openai-one"));
    config.translation.speaker_targets[0].target_language = "tlh-Latn".into();
    assert!(config.validate_settings().is_ok());

    config.translation.speaker_targets[0].target_language = "not a language".into();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Invalid speaker translation target language: not a language"
    );
}

#[test]
fn gemini_profiles_support_asr_and_still_require_an_llm_model() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "gemini-one".into(),
        name: "Gemini".into(),
        provider: GEMINI_PROVIDER.into(),
        region: None,
        workspace_id: None,
        base_url: None,
        enabled_capabilities: text_capabilities(),
        ..ApiProfile::default()
    });
    config.translation.mode = "manual".into();
    set_translation_profile(&mut config, Some("gemini-one"));
    config.translation.speaker_targets[0].model = "gemini-2.5-flash".into();
    config.translation.microphone_targets[0].model = "gemini-2.5-flash".into();
    assert!(config.validate_settings().is_ok());

    config.translation.speaker_targets[0].model.clear();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The LLM translation model cannot be empty"
    );
    config.translation.speaker_targets[0].model = "gemini-2.5-flash".into();
    config.asr.api_profiles[0]
        .enabled_capabilities
        .push(CAPABILITY_SPEECH_TO_TEXT.into());
    config.asr.backend = SERVICE_GEMINI_TRANSCRIBE.into();
    config.asr.active_profile_id = Some("gemini-one".into());
    assert!(config.validate_settings().is_ok());
}

#[test]
fn api_profile_capabilities_separate_asr_and_translation() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "openai-asr".into(),
        name: "OpenAI ASR".into(),
        provider: OPENAI_PROVIDER.into(),
        enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
        ..ApiProfile::default()
    });
    config.asr.backend = SERVICE_OPENAI_REALTIME.into();
    config.asr.active_profile_id = Some("openai-asr".into());
    assert!(config.validate_settings().is_ok());

    config.translation.mode = "manual".into();
    set_translation_profile(&mut config, Some("openai-asr"));
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The selected API profile does not support translation"
    );

    config.translation.mode = "disabled".into();
    set_translation_profile(&mut config, None);
    config.asr.api_profiles[0].enabled_capabilities = text_capabilities();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The active API profile has not enabled speech recognition"
    );
}

#[test]
fn effective_purpose_is_derived_from_capabilities() {
    let mut profile = ApiProfile {
        provider: OPENAI_PROVIDER.into(),
        enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
        ..ApiProfile::default()
    };
    assert_eq!(providers::effective_purpose(&profile), "asr");
    profile.enabled_capabilities = text_capabilities();
    assert_eq!(providers::effective_purpose(&profile), "llm");
    profile
        .enabled_capabilities
        .push(CAPABILITY_SPEECH_TO_TEXT.into());
    assert_eq!(providers::effective_purpose(&profile), "shared");
}

#[test]
fn automatic_translation_requires_a_profile() {
    let mut config = AppConfig::default();
    config.translation.mode = "automatic".into();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "A translation API profile must be selected for zh-Hans"
    );
}

#[test]
fn translation_routes_are_bounded_and_unique_per_source() {
    let mut config = AppConfig::default();
    config
        .translation
        .speaker_targets
        .push(crate::config::TranslationTargetConfig::new("zh-Hans"));
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Translation speaker target languages must be unique"
    );

    config.translation.speaker_targets = vec![
        crate::config::TranslationTargetConfig::new("en"),
        crate::config::TranslationTargetConfig::new("ja"),
        crate::config::TranslationTargetConfig::new("fr"),
        crate::config::TranslationTargetConfig::new("de"),
    ];
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Translation speaker targets must contain between 1 and 3 entries"
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
        base_url: Some("https://example.com/v1".into()),
        enabled_capabilities: text_capabilities(),
        ..ApiProfile::default()
    });
    config.translation.mode = "manual".into();
    set_translation_profile(&mut config, Some("deepseek"));
    config.translation.speaker_targets[0].model = "deepseek-chat".into();
    config.translation.microphone_targets[0].model = "deepseek-chat".into();
    assert!(config.validate_settings().is_ok());

    config.asr.backend = SERVICE_OPENAI_REALTIME.into();
    config.asr.active_profile_id = Some("deepseek".into());
    assert!(config.validate_settings().is_err());

    config.asr.active_profile_id = None;
    config.asr.api_profiles[0].base_url = Some("https://example.com/v1?token=secret".into());
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The API profile Base URL cannot contain credentials, a query, or a fragment"
    );
}

#[test]
fn validates_compatible_timeout_and_safe_headers() {
    let mut config = AppConfig::default();
    config.asr.api_profiles.push(ApiProfile {
        id: "local".into(),
        name: "Local".into(),
        provider: OLLAMA_PROVIDER.into(),
        base_url: None,
        enabled_capabilities: text_capabilities(),
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
        enabled_capabilities: text_capabilities(),
        auth_mode: ApiAuthMode::Bearer,
        ..ApiProfile::default()
    });

    assert_eq!(
        config.validate_settings().unwrap_err(),
        "API profiles cannot send Bearer credentials over remote HTTP"
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
    let mut glossary = GlossaryConfig {
        sources: vec![
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
        ],
        ..Default::default()
    };
    assert!(validate_glossary(&glossary).is_ok());

    if let GlossarySource::Subscription { url, .. } = &mut glossary.sources[1] {
        *url = "http://127.0.0.1:8080/glossary.json".into();
    }
    assert!(validate_glossary(&glossary).is_ok());
    if let GlossarySource::Subscription { url, .. } = &mut glossary.sources[1] {
        *url = "http://example.com/glossary.json".into();
    }
    assert_eq!(
        validate_glossary(&glossary).unwrap_err(),
        "Glossary source URL must use HTTPS, except for loopback HTTP addresses"
    );

    if let GlossarySource::Subscription { id, url, .. } = &mut glossary.sources[1] {
        *id = " local ".into();
        *url = "https://example.com/glossary.json".into();
    }
    assert_eq!(
        validate_glossary(&glossary).unwrap_err(),
        "Glossary source id must be unique: local"
    );

    if let GlossarySource::Local { entries, .. } = &mut glossary.sources[0] {
        entries.push(entry("vrchat", false));
    }
    glossary.sources.pop();
    assert_eq!(
        validate_glossary(&glossary).unwrap_err(),
        "Local glossary contains a duplicate source term: vrchat"
    );

    if let GlossarySource::Local { entries, .. } = &mut glossary.sources[0] {
        entries[1].case_sensitive = true;
    }
    assert!(validate_glossary(&glossary).is_ok());
    if let GlossarySource::Local { entries, .. } = &mut glossary.sources[0] {
        entries[1].source = "line\nbreak".into();
    }
    assert_eq!(
        validate_glossary(&glossary).unwrap_err(),
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
            enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
            ..ApiProfile::default()
        },
        ApiProfile {
            id: "alibaba-two".into(),
            name: "personal".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("singapore".into()),
            workspace_id: Some("workspace-two".into()),
            base_url: None,
            enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
            ..ApiProfile::default()
        },
    ];
    assert!(config.validate_settings().is_err());

    config.asr.api_profiles[1].name = "Work".into();
    config.asr.backend = SERVICE_OPENAI_REALTIME.into();
    config.asr.active_profile_id = Some("alibaba-two".into());
    assert!(config.validate_settings().is_err());

    config.asr.backend = "qwen_realtime".into();
    assert!(config.validate_settings().is_ok());
}

#[test]
fn groq_transcription_requires_a_speech_enabled_groq_profile() {
    let mut config = AppConfig::default();
    config.asr.backend = SERVICE_GROQ_TRANSCRIPTION.into();
    config.asr.active_profile_id = Some("groq".into());
    config.asr.api_profiles.push(ApiProfile {
        id: "groq".into(),
        name: "Groq".into(),
        provider: GROQ_PROVIDER.into(),
        enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
        ..ApiProfile::default()
    });
    assert!(config.validate_settings().is_ok());

    config.asr.api_profiles[0].enabled_capabilities = text_capabilities();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "The active API profile has not enabled speech recognition"
    );
}

#[test]
fn validates_fun_asr_specific_limits() {
    let mut config = AppConfig::default();
    config.asr.backend = SERVICE_FUN_ASR_REALTIME.into();
    config
        .asr
        .service_settings
        .get_mut(SERVICE_FUN_ASR_REALTIME)
        .unwrap()
        .context = "字".repeat(400);
    assert!(config.validate_settings().is_ok());

    config
        .asr
        .service_settings
        .get_mut(SERVICE_FUN_ASR_REALTIME)
        .unwrap()
        .context
        .push('字');
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Recognition service fun_asr_realtime context cannot exceed 400 characters"
    );
}

#[test]
fn accepts_custom_recognition_model_names() {
    let mut config = AppConfig::default();
    config
        .asr
        .service_settings
        .get_mut(SERVICE_QWEN_REALTIME)
        .unwrap()
        .model = "qwen3-asr-flash-realtime-2026-02-10".into();
    assert!(config.validate_settings().is_ok());

    config
        .asr
        .service_settings
        .get_mut(SERVICE_QWEN_REALTIME)
        .unwrap()
        .model = "custom-asr-model".into();
    assert!(config.validate_settings().is_ok());

    config
        .asr
        .service_settings
        .get_mut(SERVICE_QWEN_REALTIME)
        .unwrap()
        .model = "custom-asr-model ".into();
    assert_eq!(
        config.validate_settings().unwrap_err(),
        "Unsupported model for recognition service qwen_realtime: custom-asr-model "
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
