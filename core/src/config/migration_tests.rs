use super::migration::config_from_value;
use super::*;
use crate::providers::{self, API_PURPOSE_LLM, OPENAI_COMPATIBLE_PROVIDER};

#[test]
fn schema_v3_without_model_directory_uses_the_default() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 3
    }))
    .unwrap();

    assert_eq!(config.storage.model_directory, "models/whisper");
    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(config.asr.backend, "local_whisper");
}

#[test]
fn schema_v4_without_feature_switches_keeps_existing_features_enabled() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 4
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert!(config.anki.enabled);
    assert!(config.dictionary.selection_lookup_enabled);
}

#[test]
fn migrates_v1_layout() {
    let raw = serde_json::json!({
        "host": "127.0.0.1",
        "port": 9000,
        "database_path": "data/custom.db",
        "subtitle_history_limit": 100,
        "audio_device_id": 3,
        "microphone_device_id": 7,
        "vrchat_only": false,
        "asr": {"model": "tiny", "language": "ja"}
    });
    let config = config_from_value(&raw).unwrap();
    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(config.server.port, 9000);
    assert_eq!(config.storage.database_path, "data/custom.db");
    assert_eq!(config.storage.model_directory, "models/whisper");
    assert_eq!(config.storage.subtitle_history_max_bytes, 100 * 1024 * 1024);
    assert_eq!(config.audio.output.device_id, Some(3));
    assert_eq!(config.audio.microphone.mode, "device");
    assert_eq!(config.audio.microphone.device_id, Some(7));
    assert_eq!(config.asr.local.model, "tiny");
    assert_eq!(config.asr.language, "ja");
    assert_eq!(config.asr.local.device, "auto");
}

#[test]
fn migration_resolves_port_collision() {
    let raw = serde_json::json!({"port": 8765});
    let config = config_from_value(&raw).unwrap();
    assert_eq!(config.server.port, 8766);
}

#[test]
fn rejects_invalid_vad_during_migration() {
    let raw = serde_json::json!({
        "schema_version": 4,
        "vad": {"silence_seconds": 5.0}
    });
    assert!(config_from_value(&raw).is_err());
}

#[test]
fn migrates_v11_profile_transport_defaults() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(11);
    raw["asr"]["api_profiles"] = serde_json::json!([{
        "id": "compatible",
        "name": "Compatible",
        "provider": "openai_compatible",
        "base_url": "https://example.com/v1",
        "purpose": "llm"
    }]);
    let config = config_from_value(&raw).unwrap();
    let profile = &config.asr.api_profiles[0];
    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(profile.auth_mode, ApiAuthMode::Bearer);
    assert!(!profile.is_local);
    assert_eq!(profile.timeout_ms, 8_000);
    assert!(profile.preset_id.is_none());
    assert!(profile.headers.is_empty());
}

#[test]
fn migrates_v15_official_compatible_profiles_to_their_presets() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(15);
    raw["asr"]["api_profiles"] = serde_json::json!([
        {
            "id": "deepseek",
            "name": "DeepSeek",
            "provider": "openai_compatible",
            "base_url": "https://api.deepseek.com/v1/",
            "purpose": "llm"
        },
        {
            "id": "custom",
            "name": "Custom",
            "provider": "openai_compatible",
            "base_url": "https://example.com/v1",
            "purpose": "llm"
        },
        {
            "id": "lm-studio",
            "name": "LM Studio",
            "provider": "openai_compatible",
            "base_url": "http://localhost:1234/v1/",
            "purpose": "llm"
        },
        {
            "id": "ollama",
            "name": "Ollama",
            "provider": "openai_compatible",
            "base_url": "http://127.0.0.1:11434/v1",
            "purpose": "llm"
        }
    ]);

    let config = config_from_value(&raw).unwrap();

    assert_eq!(
        config.asr.api_profiles[0].preset_id.as_deref(),
        Some("deepseek")
    );
    assert!(config.asr.api_profiles[1].preset_id.is_none());
    for (profile, preset) in config.asr.api_profiles[2..]
        .iter()
        .zip(["lm_studio", "ollama"])
    {
        assert_eq!(profile.preset_id.as_deref(), Some(preset));
        assert_eq!(profile.auth_mode, ApiAuthMode::None);
        assert!(profile.is_local);
    }
}

#[test]
fn migrates_v18_history_count_to_storage_quota() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(18);
    raw["storage"]
        .as_object_mut()
        .unwrap()
        .remove("subtitle_history_max_bytes");
    raw["storage"]["subtitle_history_limit"] = serde_json::json!(500);

    let config = config_from_value(&raw).unwrap();
    let migrated = serde_json::to_value(config).unwrap();

    assert_eq!(
        migrated["schema_version"],
        serde_json::json!(SCHEMA_VERSION)
    );
    assert_eq!(
        migrated["storage"]["subtitle_history_max_bytes"],
        serde_json::json!(100_u64 * 1024 * 1024)
    );
    assert!(migrated["storage"].get("subtitle_history_limit").is_none());
}

#[test]
fn migrates_v17_glossary_fields_to_ordered_sources() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(17);
    raw["translation"]["prompt"]["glossary"] = serde_json::json!([{
        "source": "VRChat",
        "target": "VRChat",
        "category": "game",
        "case_sensitive": false
    }]);
    raw["translation"]["prompt"]["glossary_source_url"] =
        serde_json::json!("https://example.com/glossary.json");

    let config = config_from_value(&raw).unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert!(config.translation.prompt.glossary.is_empty());
    assert_eq!(config.translation.prompt.glossary_sources.len(), 2);
    assert!(matches!(
        &config.translation.prompt.glossary_sources[0],
        GlossarySource::Local { id, name, enabled: true, entries }
            if id == "legacy-local" && name == "Local glossary" && entries.len() == 1
    ));
    assert!(matches!(
        &config.translation.prompt.glossary_sources[1],
        GlossarySource::Subscription {
            id,
            url,
            display_name: None,
            enabled: true
        } if id == "legacy-subscription" && url == "https://example.com/glossary.json"
    ));
    let migrated = serde_json::to_value(config).unwrap();
    let prompt = &migrated["translation"]["prompt"];
    assert!(prompt.get("glossary").is_none());
    assert!(prompt.get("glossary_source_url").is_none());
}

#[test]
fn older_migrations_apply_the_glossary_source_backfill() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 12,
        "translation": {
            "prompt": {
                "glossary": [{"source": "Udon"}],
                "glossary_source_url": "https://example.com/remote.json"
            }
        }
    }))
    .unwrap();

    assert_eq!(config.translation.prompt.glossary_sources.len(), 2);
    assert!(matches!(
        &config.translation.prompt.glossary_sources[0],
        GlossarySource::Local { id, .. } if id == "legacy-local"
    ));
    assert!(matches!(
        &config.translation.prompt.glossary_sources[1],
        GlossarySource::Subscription { id, .. } if id == "legacy-subscription"
    ));
}

#[test]
fn migrates_v10_openai_base_urls_to_llm_only_compatible_profiles() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 10,
        "asr": {
            "api_profiles": [{
                "id": "deepseek",
                "name": "DeepSeek",
                "provider": "openai",
                "base_url": "https://api.deepseek.com/v1",
                "purpose": "shared"
            }]
        }
    }))
    .unwrap();

    let profile = &config.asr.api_profiles[0];
    assert_eq!(profile.provider, OPENAI_COMPATIBLE_PROVIDER);
    assert_eq!(profile.preset_id.as_deref(), Some("deepseek"));
    assert_eq!(providers::effective_purpose(profile), API_PURPOSE_LLM);
    assert!(!providers::supports_realtime_asr(profile));
    assert!(providers::supports_translation(profile));
}

#[test]
fn migrates_v5_provider_slots_to_named_profiles() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 5,
        "asr": {
            "qwen": {
                "region": "singapore",
                "workspace_id": "ws-example"
            }
        }
    }))
    .unwrap();

    assert_eq!(config.asr.api_profiles.len(), 2);
    let alibaba = &config.asr.api_profiles[0];
    assert_eq!(alibaba.id, "legacy-alibaba-cloud");
    assert_eq!(alibaba.region.as_deref(), Some("singapore"));
    assert_eq!(alibaba.workspace_id.as_deref(), Some("ws-example"));
    assert_eq!(
        config.asr.active_api_profiles.alibaba_cloud.as_deref(),
        Some("legacy-alibaba-cloud")
    );
}

#[test]
fn migrates_v6_with_translation_disabled_by_default() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 6
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(config.translation.mode, "disabled");
    assert_eq!(config.translation.target_language, "zh-Hans");
    assert!(!config.translation.thinking_enabled);
    assert!(!config.translation.translate_microphone);
    assert_eq!(config.translation.microphone_target_language, "zh-Hans");
}

#[test]
fn migrates_v7_with_osc_disabled_by_default() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 7
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert!(!config.osc.enabled);
    assert_eq!(config.osc.port, 9000);
    assert!(config.osc.mute_sync_enabled);
    assert!(!config.osc.mute_status_toast_enabled);
}

#[test]
fn migrates_v9_with_mute_sync_enabled_by_default() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 9,
        "osc": {
            "enabled": true,
            "port": 9001
        }
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert!(config.osc.enabled);
    assert_eq!(config.osc.port, 9001);
    assert!(config.osc.mute_sync_enabled);
    assert!(!config.osc.mute_status_toast_enabled);
}

#[test]
fn migrates_v8_automatic_translation_without_changing_behavior() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 8,
        "asr": {
            "api_profiles": [{
                "id": "deepl-one",
                "name": "DeepL",
                "provider": "deepl"
            }]
        },
        "translation": {
            "mode": "automatic",
            "target_language": "ja",
            "profile_id": "deepl-one"
        }
    }))
    .unwrap();

    assert!(config.translation.translate_microphone);
    assert_eq!(config.translation.microphone_target_language, "ja");
}

#[test]
fn migrates_v12_with_translation_context_disabled() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 12,
        "translation": {
            "mode": "disabled",
            "target_language": "ja",
            "microphone_target_language": "en"
        }
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert!(!config.translation.prompt.context_enabled);
    assert!(config.translation.prompt.include_speaker);
    assert!(config.translation.prompt.include_microphone);
    assert!(config.translation.prompt.include_chatbox);
    assert_eq!(config.translation.prompt.max_messages, 5);
    assert_eq!(config.translation.prompt.max_chars, 4_000);
    assert!(config.translation.prompt.glossary.is_empty());
    assert!(config.translation.prompt.glossary_sources.is_empty());
}

#[test]
fn migrates_v13_with_external_api_disabled_on_loopback() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 13
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert!(!config.external_api.enabled);
    assert_eq!(config.external_api.host, "127.0.0.1");
    assert_eq!(config.external_api.port, 8767);
    assert!(!config.external_api.require_token);
}

#[test]
fn rejects_non_object_and_invalid_schema_version() {
    assert!(config_from_value(&serde_json::json!([])).is_err());
    assert!(config_from_value(&serde_json::json!({"schema_version": "3"})).is_err());
}
