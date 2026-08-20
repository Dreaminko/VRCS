use super::migration::config_from_value;
use super::*;
use crate::providers::{
    self, CAPABILITY_SPEECH_TO_TEXT, CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION,
    DEEPSEEK_PROVIDER, GROQ_PROVIDER, LM_STUDIO_PROVIDER, OLLAMA_PROVIDER,
    OPENAI_COMPATIBLE_PROVIDER, OPENROUTER_PROVIDER, SERVICE_FUN_ASR_REALTIME,
    SERVICE_GROQ_TRANSCRIPTION, SERVICE_OPENAI_REALTIME, SERVICE_QWEN_REALTIME,
};

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
    assert_eq!(config.storage.subtitle_history_max_bytes, 512 * 1024 * 1024);
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
fn migrates_v15_compatible_profiles_to_independent_brands() {
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
            "id": "groq",
            "name": "Groq",
            "provider": "openai_compatible",
            "preset_id": "groq",
            "base_url": "https://example.com/v1",
            "purpose": "llm"
        },
        {
            "id": "openrouter",
            "name": "OpenRouter",
            "provider": "openai_compatible",
            "base_url": "https://openrouter.ai/api/v1",
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

    assert_eq!(config.asr.api_profiles[0].provider, DEEPSEEK_PROVIDER);
    assert_eq!(config.asr.api_profiles[1].provider, GROQ_PROVIDER);
    assert_eq!(config.asr.api_profiles[2].provider, OPENROUTER_PROVIDER);
    assert_eq!(
        config.asr.api_profiles[3].provider,
        OPENAI_COMPATIBLE_PROVIDER
    );
    for (profile, provider) in config.asr.api_profiles[4..]
        .iter()
        .zip([LM_STUDIO_PROVIDER, OLLAMA_PROVIDER])
    {
        assert_eq!(profile.provider, provider);
        assert_eq!(profile.auth_mode, ApiAuthMode::None);
        assert!(profile.is_local);
    }
    let groq = &config.asr.api_profiles[1];
    assert_eq!(groq.id, "groq");
    assert_eq!(
        groq.enabled_capabilities,
        [CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION]
    );
    assert!(!groq
        .enabled_capabilities
        .iter()
        .any(|capability| capability == CAPABILITY_SPEECH_TO_TEXT));
}

#[test]
fn migrates_v21_with_complete_vr_overlay_defaults() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(21);
    raw.as_object_mut().unwrap().remove("vr_overlay");

    let config = config_from_value(&raw).unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(config.vr_overlay, VrOverlayConfig::default());
    assert!(config.vr_overlay.headset.enabled);
    assert!(config.vr_overlay.wrist.enabled);
}

#[test]
fn migrates_v20_without_independent_microphone_translation_switch() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(20);
    raw["translation"]["mode"] = serde_json::json!("disabled");
    raw["translation"]["translate_microphone"] = serde_json::json!(true);

    let config = config_from_value(&raw).unwrap();
    let migrated = serde_json::to_value(config).unwrap();

    assert_eq!(
        migrated["schema_version"],
        serde_json::json!(SCHEMA_VERSION)
    );
    assert_eq!(
        migrated["translation"]["mode"],
        serde_json::json!("disabled")
    );
    assert!(migrated["translation"]
        .get("translate_microphone")
        .is_none());
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
        serde_json::json!(512_u64 * 1024 * 1024)
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
    assert_eq!(config.glossary.sources.len(), 2);
    assert!(matches!(
        &config.glossary.sources[0],
        GlossarySource::Local { id, name, enabled: true, entries }
            if id == "legacy-local" && name == "Local glossary" && entries.len() == 1
    ));
    assert!(matches!(
        &config.glossary.sources[1],
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
    assert!(prompt.get("glossary_sources").is_none());
    assert_eq!(migrated["glossary"]["sources"].as_array().unwrap().len(), 2);
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

    assert_eq!(config.glossary.sources.len(), 2);
    assert!(matches!(
        &config.glossary.sources[0],
        GlossarySource::Local { id, .. } if id == "legacy-local"
    ));
    assert!(matches!(
        &config.glossary.sources[1],
        GlossarySource::Subscription { id, .. } if id == "legacy-subscription"
    ));
}

#[test]
fn migrates_v10_openai_base_urls_to_llm_only_branded_profiles() {
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
    assert_eq!(profile.provider, DEEPSEEK_PROVIDER);
    assert_eq!(providers::effective_purpose(profile), "llm");
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
        config.asr.active_profile_id.as_deref(),
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
    assert_eq!(
        config.translation.speaker_targets[0].target_language,
        "zh-Hans"
    );
    assert!(!config.translation.speaker_targets[0].thinking_enabled);
    assert_eq!(
        config.translation.microphone_targets[0].target_language,
        "zh-Hans"
    );
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
    assert!(config.osc.preserve_original_text);
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
    assert!(config.osc.preserve_original_text);
}

#[test]
fn migrates_v8_automatic_translation_target_for_microphone() {
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

    assert_eq!(config.translation.mode, "automatic");
    assert_eq!(
        config.translation.microphone_targets[0].target_language,
        "ja"
    );
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
    assert!(config.glossary.sources.is_empty());
}

#[test]
fn migrates_v25_translation_fields_into_ordered_routes() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 25,
        "translation": {
            "mode": "disabled",
            "target_language": "ja",
            "microphone_target_language": "en",
            "profile_id": null,
            "model": "legacy-model",
            "thinking_enabled": true
        }
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(config.translation.speaker_targets.len(), 1);
    assert_eq!(config.translation.speaker_targets[0].target_language, "ja");
    assert_eq!(config.translation.speaker_targets[0].model, "legacy-model");
    assert!(config.translation.speaker_targets[0].thinking_enabled);
    assert_eq!(
        config.translation.microphone_targets[0].target_language,
        "en"
    );
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
fn migrates_v19_with_vrcx_disabled() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 19
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert!(!config.vrcx.enabled);
    assert_eq!(config.vrcx.port, 8799);
    assert!(!config.vrcx.include_in_llm_context);
    assert!(!config.vrcx.include_in_asr_context);
}

#[test]
fn migrates_v22_with_the_default_output_trigger_threshold() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 22
    }))
    .unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(config.audio.output.trigger_threshold_dbfs, -45.0);
}

#[test]
fn migrates_v24_glossary_sources_to_top_level_without_data_loss() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(24);
    raw["translation"]["prompt"]["glossary_sources"] = serde_json::json!([
        {
            "id": "local",
            "type": "local",
            "name": "Local names",
            "enabled": false,
            "entries": [{
                "source": "VRChat",
                "target": "VRChat",
                "category": "game",
                "case_sensitive": true
            }]
        },
        {
            "id": "remote",
            "type": "subscription",
            "url": "https://example.com/glossary.json",
            "display_name": "Community",
            "enabled": true
        }
    ]);

    let config = config_from_value(&raw).unwrap();

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert!(config.glossary.llm_enabled);
    assert!(config.glossary.asr_enabled);
    assert_eq!(config.glossary.sources.len(), 2);
    assert!(matches!(
        &config.glossary.sources[0],
        GlossarySource::Local { id, name, enabled: false, entries }
            if id == "local"
                && name == "Local names"
                && entries.len() == 1
                && entries[0].source == "VRChat"
                && entries[0].target.as_deref() == Some("VRChat")
                && entries[0].category == GlossaryCategory::Game
                && entries[0].case_sensitive
    ));
    assert!(matches!(
        &config.glossary.sources[1],
        GlossarySource::Subscription {
            id,
            url,
            display_name: Some(display_name),
            enabled: true
        } if id == "remote"
            && url == "https://example.com/glossary.json"
            && display_name == "Community"
    ));
    let migrated = serde_json::to_value(config).unwrap();
    assert!(migrated["translation"]["prompt"]
        .get("glossary_sources")
        .is_none());
}

#[test]
fn migrates_v24_glossary_sources_without_overwriting_existing_top_level_sources() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(24);
    raw["glossary"]["sources"] = serde_json::json!([{
        "id": "top-level",
        "type": "local",
        "name": "Top level",
        "enabled": true,
        "entries": [{"source": "Udon"}]
    }]);
    raw["translation"]["prompt"]["glossary_sources"] = serde_json::json!([{
        "id": "nested",
        "type": "subscription",
        "url": "https://example.com/glossary.json",
        "display_name": null,
        "enabled": true
    }]);

    let config = config_from_value(&raw).unwrap();

    assert!(config.glossary.llm_enabled);
    assert!(config.glossary.asr_enabled);
    assert_eq!(config.glossary.sources.len(), 2);
    assert!(matches!(
        &config.glossary.sources[0],
        GlossarySource::Local { id, .. } if id == "top-level"
    ));
    assert!(matches!(
        &config.glossary.sources[1],
        GlossarySource::Subscription { id, .. } if id == "nested"
    ));
}

#[test]
fn migrates_v23_profiles_and_recognition_settings_through_v24_normalize() {
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(23);
    raw["asr"] = serde_json::json!({
        "backend": "fun_asr_realtime",
        "language": "ja",
        "local": {"model": "small", "device": "auto", "compute_type": "int8"},
        "qwen": {"model": "qwen3-asr-flash-realtime", "context": "qwen context"},
        "fun_asr": {"model": "fun-asr-realtime", "context": "fun context"},
        "openai": {"model": "gpt-4o-transcribe"},
        "api_profiles": [{
            "id": "alibaba-profile",
            "name": "Alibaba",
            "provider": "alibaba_cloud",
            "region": "singapore",
            "workspace_id": "workspace",
            "purpose": "shared"
        }],
        "active_api_profiles": {
            "alibaba_cloud": "alibaba-profile",
            "openai": null
        },
        "cloud_failure_policy": "reconnect"
    });

    let config = config_from_value(&raw).unwrap();
    let profile = &config.asr.api_profiles[0];

    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(
        config.asr.active_profile_id.as_deref(),
        Some("alibaba-profile")
    );
    assert_eq!(
        profile.enabled_capabilities,
        [
            CAPABILITY_TEXT_GENERATION,
            CAPABILITY_TEXT_TRANSLATION,
            CAPABILITY_SPEECH_TO_TEXT,
        ]
    );
    assert_eq!(
        config.asr.service_settings[SERVICE_QWEN_REALTIME].context,
        "qwen context"
    );
    assert_eq!(
        config.asr.service_settings[SERVICE_FUN_ASR_REALTIME].context,
        "fun context"
    );
    assert_eq!(
        config.asr.service_settings[SERVICE_OPENAI_REALTIME].model,
        "gpt-4o-transcribe"
    );
    assert_eq!(
        config.asr.service_settings[SERVICE_GROQ_TRANSCRIPTION].model,
        "whisper-large-v3-turbo"
    );
}

#[test]
fn strict_brand_detection_leaves_unofficial_urls_custom() {
    let config = config_from_value(&serde_json::json!({
        "schema_version": 23,
        "asr": {
            "backend": "local_whisper",
            "api_profiles": [{
                "id": "custom",
                "name": "Custom",
                "provider": "openai_compatible",
                "base_url": "https://api.deepseek.com.evil.example/v1",
                "purpose": "llm"
            }]
        }
    }))
    .unwrap();

    assert_eq!(
        config.asr.api_profiles[0].provider,
        OPENAI_COMPATIBLE_PROVIDER
    );
}

#[test]
fn rejects_non_object_and_invalid_schema_version() {
    assert!(config_from_value(&serde_json::json!([])).is_err());
    assert!(config_from_value(&serde_json::json!({"schema_version": "3"})).is_err());
}
