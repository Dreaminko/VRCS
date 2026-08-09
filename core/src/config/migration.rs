use serde::Deserialize;

use super::{
    default_asr_model, default_compute_type, default_device, default_language, AppConfig,
    AsrConfig, AudioConfig, LocalAsrConfig, MicrophoneConfig, OutputConfig, ServerConfig,
    StorageConfig, SCHEMA_VERSION,
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
            subtitle_history_limit: raw
                .get("subtitle_history_limit")
                .and_then(|value| value.as_u64())
                .map(|value| value as u32)
                .unwrap_or(defaults.storage.subtitle_history_limit),
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
            },
            microphone: MicrophoneConfig {
                mode: if microphone_device_id.is_some() {
                    "device".into()
                } else {
                    "disabled".into()
                },
                device_id: microphone_device_id,
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
    let mut config: AppConfig = serde_json::from_value(value).map_err(|error| error.to_string())?;
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
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn migrate_v6(raw: &serde_json::Value) -> Result<AppConfig, String> {
    let mut value = raw.clone();
    value
        .as_object_mut()
        .ok_or_else(|| "Configuration root must be an object".to_string())?
        .insert("schema_version".into(), serde_json::json!(SCHEMA_VERSION));
    serde_json::from_value(value).map_err(|error| error.to_string())
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
        6 => migrate_v6(raw)?,
        4 | 5 => migrate_v4_or_v5(raw)?,
        2 | 3 => migrate_v2_or_v3(raw)?,
        1 => migrate_v1(raw),
        other => return Err(format!("Unsupported configuration schema v{other}")),
    };
    config.schema_version = SCHEMA_VERSION;
    config.validate_settings()?;
    Ok(config)
}
