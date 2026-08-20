use serde::Deserialize;

use super::recognition::{
    default_asr_model, default_compute_type, default_device, default_language,
};
use super::{
    default_service_settings, AppConfig, AsrConfig, AudioConfig, LocalAsrConfig, MicrophoneConfig,
    OutputConfig, ServerConfig, StorageConfig, SCHEMA_VERSION,
};
use crate::providers::{
    self, CAPABILITY_SPEECH_TO_TEXT, CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION,
    DEEPSEEK_PROVIDER, GROQ_PROVIDER, LM_STUDIO_PROVIDER, OLLAMA_PROVIDER,
    OPENAI_COMPATIBLE_PROVIDER, OPENROUTER_PROVIDER, SERVICE_FUN_ASR_REALTIME,
    SERVICE_OPENAI_REALTIME, SERVICE_QWEN_REALTIME,
};

#[derive(Debug, Clone, Deserialize)]
struct LegacyAsrConfig {
    #[serde(default = "default_asr_model")]
    model: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default = "default_device")]
    device: String,
    #[serde(default = "default_compute_type")]
    compute_type: String,
}

impl Default for LegacyAsrConfig {
    fn default() -> Self {
        Self {
            model: default_asr_model(),
            language: default_language(),
            device: default_device(),
            compute_type: default_compute_type(),
        }
    }
}

fn migrate_v1(raw: &serde_json::Value) -> AppConfig {
    let defaults = AppConfig::default();
    let microphone_device_id = raw
        .get("microphone_device_id")
        .and_then(|value| value.as_i64());
    let vrchat_only = raw
        .get("vrchat_only")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let legacy_asr: LegacyAsrConfig = serde_json::from_value(
        raw.get("asr")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .unwrap_or_default();

    let mut config = AppConfig {
        server: ServerConfig {
            host: raw
                .get("host")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| defaults.server.host.clone()),
            port: raw
                .get("port")
                .and_then(|value| value.as_u64())
                .map(|value| value as u16)
                .unwrap_or(defaults.server.port),
        },
        storage: StorageConfig {
            database_path: raw
                .get("database_path")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| defaults.storage.database_path.clone()),
            model_directory: raw
                .get("model_directory")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| defaults.storage.model_directory.clone()),
            subtitle_history_max_bytes: defaults.storage.subtitle_history_max_bytes,
        },
        audio: AudioConfig {
            sample_rate: raw
                .get("sample_rate")
                .and_then(|value| value.as_u64())
                .map(|value| value as u32)
                .unwrap_or(defaults.audio.sample_rate),
            output: OutputConfig {
                mode: if vrchat_only {
                    "vrchat".into()
                } else {
                    "system".into()
                },
                device_id: if vrchat_only {
                    None
                } else {
                    raw.get("audio_device_id").and_then(|value| value.as_i64())
                },
                ..OutputConfig::default()
            },
            microphone: MicrophoneConfig {
                mode: if microphone_device_id.is_some() {
                    "device".into()
                } else {
                    "disabled".into()
                },
                device_id: microphone_device_id,
                ..MicrophoneConfig::default()
            },
        },
        asr: asr_from_legacy(legacy_asr),
        ..AppConfig::default()
    };
    fix_colliding_ports(&mut config);
    config
}

fn asr_from_legacy(legacy: LegacyAsrConfig) -> AsrConfig {
    AsrConfig {
        backend: "local_whisper".into(),
        language: legacy.language,
        local: LocalAsrConfig {
            model: legacy.model,
            device: legacy.device,
            compute_type: legacy.compute_type,
        },
        ..AsrConfig::default()
    }
}

fn migrate_v2_or_v3(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let legacy: LegacyAsrConfig = serde_json::from_value(
        raw.get("asr")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|error| error.to_string())?;
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    object.insert(
        "asr".into(),
        serde_json::to_value(asr_from_legacy(legacy)).map_err(|error| error.to_string())?,
    );
    backfill_glossary_sources(object)?;
    let mut config = deserialize_v24(value)?;
    if raw.get("schema_version").and_then(|value| value.as_u64()) == Some(2) {
        fix_colliding_ports(&mut config);
    }
    Ok(config)
}

fn migrate_v4_or_v5(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    let asr = object
        .entry("asr")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration asr must be an object".to_string())?;
    let qwen = asr
        .entry("qwen")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration asr.qwen must be an object".to_string())?;
    let region = qwen
        .remove("region")
        .unwrap_or_else(|| serde_json::json!("china_beijing"));
    let workspace_id = qwen
        .remove("workspace_id")
        .unwrap_or_else(|| serde_json::json!(""));
    asr.insert(
        "api_profiles".into(),
        serde_json::json!([
            {
                "id": "legacy-alibaba-cloud",
                "name": "Alibaba Cloud",
                "provider": "alibaba_cloud",
                "region": region,
                "workspace_id": workspace_id
            },
            {
                "id": "legacy-openai",
                "name": "OpenAI",
                "provider": "openai"
            }
        ]),
    );
    asr.insert(
        "active_api_profiles".into(),
        serde_json::json!({
            "alibaba_cloud": "legacy-alibaba-cloud",
            "openai": "legacy-openai"
        }),
    );
    backfill_glossary_sources(object)?;
    deserialize_v24(value)
}

fn migrate_v6_to_v10(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    let translation = object
        .entry("translation")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration translation must be an object".to_string())?;
    let microphone_target = translation
        .get("target_language")
        .cloned()
        .unwrap_or_else(|| serde_json::json!("zh-Hans"));
    translation
        .entry("microphone_target_language")
        .or_insert(microphone_target);
    if let Some(profiles) = object
        .get_mut("asr")
        .and_then(|asr| asr.get_mut("api_profiles"))
        .and_then(|profiles| profiles.as_array_mut())
    {
        for profile in profiles {
            let Some(profile) = profile.as_object_mut() else {
                continue;
            };
            let is_compatible = profile.get("provider").and_then(|value| value.as_str())
                == Some("openai")
                && profile
                    .get("base_url")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| !value.trim().is_empty());
            if is_compatible {
                profile.insert("provider".into(), serde_json::json!("openai_compatible"));
                profile.insert("purpose".into(), serde_json::json!("llm"));
            }
        }
    }
    backfill_compatible_preset(object);
    backfill_glossary_sources(object)?;
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    deserialize_v24(value)
}

fn migrate_v11_to_v17(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    backfill_compatible_preset(object);
    backfill_glossary_sources(object)?;
    migrate_storage_quota(object)?;
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    deserialize_v24(value)
}

fn migrate_v18(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    migrate_storage_quota(object)?;
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    deserialize_v24(value)
}

fn migrate_v19(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    deserialize_v24(value)
}

fn migrate_v20(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    if let Some(translation) = object
        .get_mut("translation")
        .and_then(serde_json::Value::as_object_mut)
    {
        translation.remove("translate_microphone");
    }
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    deserialize_v24(value)
}

fn migrate_v21(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    object.insert(
        "vr_overlay".into(),
        serde_json::to_value(super::VrOverlayConfig::default())
            .map_err(|error| error.to_string())?,
    );
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    deserialize_v24(value)
}

fn migrate_v22(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    deserialize_v24(value)
}

fn deserialize_v24(mut value: serde_json::Value) -> Result<AppConfig, String> {
    normalize_v24(&mut value)?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn normalize_v24(value: &mut serde_json::Value) -> Result<(), String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    promote_glossary_sources(object)?;
    normalize_translation_targets(object)?;
    let asr = object
        .entry("asr")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration asr must be an object".to_string())?;

    normalize_profiles(asr)?;
    normalize_recognition_settings(asr)?;
    object.insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    Ok(())
}

fn normalize_translation_targets(
    object: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let translation = object
        .entry("translation")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration translation must be an object".to_string())?;
    if translation.contains_key("speaker_targets") {
        return Ok(());
    }

    let target_language = translation
        .remove("target_language")
        .unwrap_or_else(|| serde_json::json!("zh-Hans"));
    let microphone_target_language = translation
        .remove("microphone_target_language")
        .unwrap_or_else(|| target_language.clone());
    let profile_id = translation
        .remove("profile_id")
        .unwrap_or(serde_json::Value::Null);
    let model = translation
        .remove("model")
        .unwrap_or_else(|| serde_json::json!("gpt-5-mini"));
    let thinking_enabled = translation
        .remove("thinking_enabled")
        .unwrap_or(serde_json::Value::Bool(false));
    let route = |language: serde_json::Value| {
        serde_json::json!({
            "target_language": language,
            "profile_id": profile_id.clone(),
            "model": model.clone(),
            "thinking_enabled": thinking_enabled.clone()
        })
    };
    translation.insert(
        "speaker_targets".into(),
        serde_json::Value::Array(vec![route(target_language)]),
    );
    translation.insert(
        "microphone_targets".into(),
        serde_json::Value::Array(vec![route(microphone_target_language)]),
    );
    Ok(())
}

fn normalize_profiles(asr: &mut serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let profiles = asr
        .entry("api_profiles")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "Configuration asr.api_profiles must be an array".to_string())?;
    for profile in profiles {
        let profile = profile
            .as_object_mut()
            .ok_or_else(|| "Configuration API profiles must be objects".to_string())?;
        let legacy_purpose = profile
            .remove("purpose")
            .and_then(|value| value.as_str().map(str::to_owned));
        extract_branded_provider(profile);
        normalize_profile_capabilities(profile, legacy_purpose.as_deref());
    }
    Ok(())
}

fn extract_branded_provider(profile: &mut serde_json::Map<String, serde_json::Value>) {
    let provider = profile
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let explicit_brand = match provider {
        GROQ_PROVIDER => Some(GROQ_PROVIDER),
        OPENROUTER_PROVIDER => Some(OPENROUTER_PROVIDER),
        DEEPSEEK_PROVIDER => Some(DEEPSEEK_PROVIDER),
        LM_STUDIO_PROVIDER => Some(LM_STUDIO_PROVIDER),
        OLLAMA_PROVIDER => Some(OLLAMA_PROVIDER),
        _ => None,
    };
    let preset = profile.get("preset_id").and_then(serde_json::Value::as_str);
    let base_url = profile.get("base_url").and_then(serde_json::Value::as_str);
    let brand = explicit_brand.or_else(|| {
        matches!(provider, OPENAI_COMPATIBLE_PROVIDER | "openai")
            .then(|| infer_branded_provider(preset, base_url))
            .flatten()
    });
    let Some(brand) = brand else {
        if provider == "openai" && base_url.is_some_and(|value| !value.trim().is_empty()) {
            profile.insert(
                "provider".into(),
                serde_json::json!(OPENAI_COMPATIBLE_PROVIDER),
            );
        }
        return;
    };
    profile.insert("provider".into(), serde_json::json!(brand));
    match brand {
        LM_STUDIO_PROVIDER | OLLAMA_PROVIDER => {
            profile.insert("auth_mode".into(), serde_json::json!("none"));
            profile.insert("is_local".into(), serde_json::json!(true));
        }
        _ => {
            profile.remove("base_url");
            profile.insert("auth_mode".into(), serde_json::json!("bearer"));
            profile.insert("is_local".into(), serde_json::json!(false));
        }
    }
}

fn normalize_profile_capabilities(
    profile: &mut serde_json::Map<String, serde_json::Value>,
    legacy_purpose: Option<&str>,
) {
    if profile.contains_key("enabled_capabilities") {
        return;
    }
    let provider = profile
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let supported = providers::provider_capability_ids(provider).unwrap_or_default();
    let include = |capability: &&str| match legacy_purpose {
        Some("asr") => *capability == CAPABILITY_SPEECH_TO_TEXT,
        Some("llm") => matches!(
            *capability,
            CAPABILITY_TEXT_GENERATION | CAPABILITY_TEXT_TRANSLATION
        ),
        Some("shared") | None => true,
        Some(_) => false,
    };
    let capabilities = supported
        .into_iter()
        .filter(include)
        .map(serde_json::Value::from)
        .collect();
    profile.insert(
        "enabled_capabilities".into(),
        serde_json::Value::Array(capabilities),
    );
}

fn normalize_recognition_settings(
    asr: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let backend = asr
        .get("backend")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(SERVICE_QWEN_REALTIME)
        .to_owned();
    if !asr.contains_key("active_profile_id") {
        let active_id = asr
            .get("active_api_profiles")
            .and_then(serde_json::Value::as_object)
            .and_then(|active| match backend.as_str() {
                SERVICE_QWEN_REALTIME | SERVICE_FUN_ASR_REALTIME => active.get("alibaba_cloud"),
                SERVICE_OPENAI_REALTIME => active.get("openai"),
                _ => None,
            })
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        asr.insert("active_profile_id".into(), active_id);
    }

    let mut settings = serde_json::to_value(default_service_settings())
        .map_err(|error| error.to_string())?
        .as_object()
        .cloned()
        .expect("default recognition settings serialize as an object");
    if let Some(existing) = asr
        .get("service_settings")
        .and_then(serde_json::Value::as_object)
    {
        for (service, value) in existing {
            settings.insert(service.clone(), value.clone());
        }
    }
    for (legacy, service) in [
        ("qwen", SERVICE_QWEN_REALTIME),
        ("fun_asr", SERVICE_FUN_ASR_REALTIME),
        ("openai", SERVICE_OPENAI_REALTIME),
    ] {
        if let Some(legacy_value) = asr.get(legacy).and_then(serde_json::Value::as_object) {
            let target = settings
                .entry(service.to_string())
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
                .expect("default recognition service settings are objects");
            if let Some(model) = legacy_value.get("model") {
                target.insert("model".into(), model.clone());
            }
            if let Some(context) = legacy_value.get("context") {
                target.insert("context".into(), context.clone());
            }
        }
    }
    asr.insert(
        "service_settings".into(),
        serde_json::Value::Object(settings),
    );
    asr.remove("active_api_profiles");
    asr.remove("qwen");
    asr.remove("fun_asr");
    asr.remove("openai");
    Ok(())
}

fn migrate_storage_quota(
    object: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let storage = object
        .entry("storage")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration storage must be an object".to_string())?;
    storage.remove("subtitle_history_limit");
    storage
        .entry("subtitle_history_max_bytes")
        .or_insert_with(|| serde_json::json!(super::runtime::DEFAULT_HISTORY_MAX_BYTES));
    Ok(())
}

fn backfill_glossary_sources(
    object: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let translation = object
        .entry("translation")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration translation must be an object".to_string())?;
    let prompt = translation
        .entry("prompt")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration translation.prompt must be an object".to_string())?;
    let glossary = prompt.remove("glossary");
    let subscription_url = prompt.remove("glossary_source_url");
    let existing_sources = prompt
        .remove("glossary_sources")
        .unwrap_or_else(|| serde_json::json!([]))
        .as_array()
        .cloned()
        .ok_or_else(|| {
            "Configuration translation.prompt.glossary_sources must be an array".to_string()
        })?;
    let mut sources = Vec::new();
    if let Some(entries) = glossary {
        if !entries.as_array().is_some_and(Vec::is_empty) {
            sources.push(serde_json::json!({
                "id": "legacy-local",
                "type": "local",
                "name": "Local glossary",
                "enabled": true,
                "entries": entries
            }));
        }
    }
    if let Some(url) = subscription_url.filter(|value| !value.is_null()) {
        sources.push(serde_json::json!({
            "id": "legacy-subscription",
            "type": "subscription",
            "url": url,
            "display_name": null,
            "enabled": true
        }));
    }
    sources.extend(existing_sources);
    prompt.insert("glossary_sources".into(), serde_json::Value::Array(sources));
    Ok(())
}

fn promote_glossary_sources(
    object: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let sources = object
        .get_mut("translation")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|translation| translation.get_mut("prompt"))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|prompt| prompt.remove("glossary_sources"));
    if sources.as_ref().is_some_and(|sources| !sources.is_array()) {
        return Err("Configuration translation.prompt.glossary_sources must be an array".into());
    }
    let glossary = object
        .entry("glossary")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "Configuration glossary must be an object".to_string())?;
    let existing_sources = glossary
        .entry("sources")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "Configuration glossary.sources must be an array".to_string())?;
    if let Some(sources) = sources {
        existing_sources.extend(
            sources
                .as_array()
                .expect("validated glossary sources")
                .iter()
                .cloned(),
        );
    }
    Ok(())
}

fn backfill_compatible_preset(object: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(profiles) = object
        .get_mut("asr")
        .and_then(|asr| asr.get_mut("api_profiles"))
        .and_then(|profiles| profiles.as_array_mut())
    else {
        return;
    };
    for profile in profiles {
        let Some(profile) = profile.as_object_mut() else {
            continue;
        };
        let is_compatible =
            profile.get("provider").and_then(|value| value.as_str()) == Some("openai_compatible");
        let has_preset = profile
            .get("preset_id")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty());
        if !is_compatible || has_preset {
            continue;
        }
        let Some(preset) = profile
            .get("base_url")
            .and_then(|value| value.as_str())
            .and_then(infer_compatible_preset)
        else {
            continue;
        };
        profile.insert("preset_id".into(), serde_json::json!(preset));
        if matches!(preset, "lm_studio" | "ollama") {
            profile.insert("auth_mode".into(), serde_json::json!("none"));
            profile.insert("is_local".into(), serde_json::json!(true));
        }
    }
}

fn infer_compatible_preset(value: &str) -> Option<&'static str> {
    infer_branded_provider(None, Some(value))
}

fn infer_branded_provider(preset: Option<&str>, base_url: Option<&str>) -> Option<&'static str> {
    if let Some(provider) = match preset.map(str::trim) {
        Some("groq") => Some(GROQ_PROVIDER),
        Some("openrouter") => Some(OPENROUTER_PROVIDER),
        Some("deepseek") => Some(DEEPSEEK_PROVIDER),
        Some("lm_studio") => Some(LM_STUDIO_PROVIDER),
        Some("ollama") => Some(OLLAMA_PROVIDER),
        _ => None,
    } {
        return Some(provider);
    }
    let value = base_url?;
    if is_official_provider_base_url(
        value,
        "api.deepseek.com",
        &["", "/v1", "/v1/chat/completions"],
    ) {
        return Some(DEEPSEEK_PROVIDER);
    }
    if is_official_provider_base_url(
        value,
        "api.groq.com",
        &["", "/openai/v1", "/openai/v1/chat/completions"],
    ) {
        return Some(GROQ_PROVIDER);
    }
    if is_official_provider_base_url(
        value,
        "openrouter.ai",
        &["/api/v1", "/api/v1/chat/completions"],
    ) {
        return Some(OPENROUTER_PROVIDER);
    }
    let url = reqwest::Url::parse(value.trim()).ok()?;
    let host = url.host_str()?;
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    let path = url.path().trim_end_matches('/');
    if url.scheme() != "http"
        || !is_loopback
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(path, "" | "/v1" | "/v1/chat/completions")
    {
        return None;
    }
    match url.port() {
        Some(1234) => Some(LM_STUDIO_PROVIDER),
        Some(11434) => Some(OLLAMA_PROVIDER),
        _ => None,
    }
}

fn is_official_provider_base_url(value: &str, host: &str, paths: &[&str]) -> bool {
    let Ok(url) = reqwest::Url::parse(value.trim()) else {
        return false;
    };
    let path = url.path().trim_end_matches('/');
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|value| value.eq_ignore_ascii_case(host))
        && url.port_or_known_default() == Some(443)
        && url.query().is_none()
        && url.fragment().is_none()
        && paths.contains(&path)
}

/// v1/v2 迁移共用：避免 Core 端口与 AnkiConnect 默认端口冲突。
fn fix_colliding_ports(config: &mut AppConfig) {
    if config.server.port == 8765 {
        config.server.port = 8766;
    }
    if config.anki.port == 8766 {
        config.anki.port = 8765;
    }
}

pub(super) fn config_version(raw: &serde_json::Value) -> Result<u64, String> {
    let object = raw
        .as_object()
        .ok_or_else(|| "Configuration root must be an object".to_string())?;
    match object.get("schema_version") {
        None => Ok(1),
        Some(value) => value
            .as_u64()
            .ok_or_else(|| "Configuration schema_version must be an integer".to_string()),
    }
}

pub fn config_from_value(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let version = config_version(raw)?;
    let mut config = match version {
        version if version == SCHEMA_VERSION as u64 => {
            serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?
        }
        23..=25 => deserialize_v24(raw.clone())?,
        22 => migrate_v22(raw)?,
        21 => migrate_v21(raw)?,
        20 => migrate_v20(raw)?,
        19 => migrate_v19(raw)?,
        18 => migrate_v18(raw)?,
        11..=17 => migrate_v11_to_v17(raw)?,
        6..=10 => migrate_v6_to_v10(raw)?,
        4 | 5 => migrate_v4_or_v5(raw)?,
        2 | 3 => migrate_v2_or_v3(raw)?,
        1 => deserialize_v24(
            serde_json::to_value(migrate_v1(raw)).map_err(|error| error.to_string())?,
        )?,
        other => return Err(format!("Unsupported configuration schema v{other}")),
    };
    config.schema_version = SCHEMA_VERSION;
    config.validate_settings()?;
    Ok(config)
}
