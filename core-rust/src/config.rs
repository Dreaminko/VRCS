//! 服务配置：JSON 文件读写与 schema v1→v3 迁移。
//! 与 Python 版 `app/config.py` 行为保持一致。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_database_path")]
    pub database_path: String,
    #[serde(default = "default_model_directory")]
    pub model_directory: String,
    #[serde(default = "default_history_limit")]
    pub subtitle_history_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_output_mode")]
    pub mode: String,
    #[serde(default)]
    pub device_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicrophoneConfig {
    #[serde(default = "default_microphone_mode")]
    pub mode: String,
    #[serde(default)]
    pub device_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioConfig {
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub microphone: MicrophoneConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VadConfig {
    #[serde(default = "default_silence_seconds")]
    pub silence_seconds: f64,
    #[serde(default = "default_max_speech_seconds")]
    pub max_speech_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsrConfig {
    #[serde(default = "default_asr_model")]
    pub model: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_compute_type")]
    pub compute_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnkiConfig {
    #[serde(default = "default_anki_port")]
    pub port: u16,
    #[serde(default = "default_anki_deck")]
    pub deck: String,
    #[serde(default = "default_anki_model")]
    pub model: String,
    #[serde(default = "default_front_field")]
    pub front_field: String,
    #[serde(default = "default_back_field")]
    pub back_field: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub vad: VadConfig,
    #[serde(default)]
    pub asr: AsrConfig,
    #[serde(default)]
    pub anki: AnkiConfig,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}
fn default_host() -> String {
    "127.0.0.1".into()
}
fn default_port() -> u16 {
    8766
}
fn default_database_path() -> String {
    "data/vrcs.db".into()
}
fn default_model_directory() -> String {
    "models/whisper".into()
}
fn default_history_limit() -> u32 {
    500
}
fn default_output_mode() -> String {
    "system".into()
}
fn default_microphone_mode() -> String {
    "disabled".into()
}
fn default_sample_rate() -> u32 {
    16_000
}
fn default_silence_seconds() -> f64 {
    0.4
}
fn default_max_speech_seconds() -> f64 {
    6.0
}
fn default_asr_model() -> String {
    "small".into()
}
fn default_language() -> String {
    "auto".into()
}
fn default_device() -> String {
    "auto".into()
}
fn default_compute_type() -> String {
    "int8".into()
}
fn default_anki_port() -> u16 {
    8765
}
fn default_anki_deck() -> String {
    "VRCS".into()
}
fn default_anki_model() -> String {
    "Basic".into()
}
fn default_front_field() -> String {
    "Front".into()
}
fn default_back_field() -> String {
    "Back".into()
}

macro_rules! impl_default {
    ($ty:ty, { $($field:ident : $default:expr),* $(,)? }) => {
        impl Default for $ty {
            fn default() -> Self {
                Self { $($field: $default()),* }
            }
        }
    };
}

impl_default!(ServerConfig, { host: default_host, port: default_port });
impl_default!(StorageConfig, {
    database_path: default_database_path,
    model_directory: default_model_directory,
    subtitle_history_limit: default_history_limit,
});
impl_default!(OutputConfig, { mode: default_output_mode, device_id: Option::default });
impl_default!(MicrophoneConfig, { mode: default_microphone_mode, device_id: Option::default });
impl_default!(AudioConfig, {
    sample_rate: default_sample_rate,
    output: OutputConfig::default,
    microphone: MicrophoneConfig::default,
});
impl_default!(VadConfig, {
    silence_seconds: default_silence_seconds,
    max_speech_seconds: default_max_speech_seconds,
});
impl_default!(AsrConfig, {
    model: default_asr_model,
    language: default_language,
    device: default_device,
    compute_type: default_compute_type,
});
impl_default!(AnkiConfig, {
    port: default_anki_port,
    deck: default_anki_deck,
    model: default_anki_model,
    front_field: default_front_field,
    back_field: default_back_field,
});
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
            audio: AudioConfig::default(),
            vad: VadConfig::default(),
            asr: AsrConfig::default(),
            anki: AnkiConfig::default(),
        }
    }
}

impl VadConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.1..=2.0).contains(&self.silence_seconds) {
            return Err("VAD silence_seconds must be between 0.1 and 2.0".into());
        }
        if !(1.0..=30.0).contains(&self.max_speech_seconds) {
            return Err("VAD max_speech_seconds must be between 1.0 and 30.0".into());
        }
        Ok(())
    }
}

const ASR_LANGUAGES: [&str; 8] = ["auto", "en", "ja", "zh", "ko", "es", "fr", "de"];
const ASR_DEVICES: [&str; 3] = ["auto", "cpu", "cuda"];
const ASR_COMPUTE_TYPES: [&str; 1] = ["int8"];

impl AppConfig {
    /// 配置文件与 PUT /api/settings 共用的完整结构校验。
    pub fn validate_settings(&self) -> Result<(), String> {
        if self.server.port == 0 {
            return Err("端口必须在 1 到 65535 之间".into());
        }
        if !(1..=10_000).contains(&self.storage.subtitle_history_limit) {
            return Err("subtitle_history_limit 必须在 1 到 10000 之间".into());
        }
        if self.storage.model_directory.trim().is_empty() {
            return Err("模型保存位置不能为空".into());
        }
        if !(8_000..=96_000).contains(&self.audio.sample_rate) {
            return Err("采样率必须在 8000 到 96000 之间".into());
        }
        match self.audio.output.mode.as_str() {
            "system" => {}
            "vrchat" if self.audio.output.device_id.is_some() => {
                return Err("VRChat 模式不能指定系统输出设备".into());
            }
            "vrchat" => {}
            other => return Err(format!("不支持的输出模式：{other}")),
        }
        match self.audio.microphone.mode.as_str() {
            "device" if self.audio.microphone.device_id.is_none() => {
                return Err("指定麦克风模式必须选择设备".into());
            }
            "device" => {}
            "default" | "disabled" if self.audio.microphone.device_id.is_some() => {
                return Err("默认或关闭麦克风模式不能指定设备".into());
            }
            "default" | "disabled" => {}
            other => return Err(format!("不支持的麦克风模式：{other}")),
        }
        self.vad.validate()?;
        if !ASR_LANGUAGES.contains(&self.asr.language.as_str()) {
            return Err(format!("不支持的识别语言：{}", self.asr.language));
        }
        if !ASR_DEVICES.contains(&self.asr.device.as_str()) {
            return Err(format!("不支持的识别设备：{}", self.asr.device));
        }
        if !ASR_COMPUTE_TYPES.contains(&self.asr.compute_type.as_str()) {
            return Err(format!("不支持的计算类型：{}", self.asr.compute_type));
        }
        if self.anki.port == 0 {
            return Err("AnkiConnect 端口必须在 1 到 65535 之间".into());
        }
        for (label, value) in [
            ("牌组", &self.anki.deck),
            ("笔记类型", &self.anki.model),
            ("正面字段", &self.anki.front_field),
            ("背面字段", &self.anki.back_field),
        ] {
            if value.is_empty() || value.chars().count() > 100 {
                return Err(format!("Anki {label}名称必须在 1 到 100 字符之间"));
            }
        }
        if self.anki.front_field == self.anki.back_field {
            return Err("Anki 正面和背面不能映射到同一个字段".into());
        }
        Ok(())
    }
}

fn migrate_v1(raw: &serde_json::Value) -> AppConfig {
    let defaults = AppConfig::default();
    let microphone_device_id = raw.get("microphone_device_id").and_then(|v| v.as_i64());
    let vrchat_only = raw
        .get("vrchat_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let asr_raw = raw.get("asr").cloned().unwrap_or_default();

    let mut config = AppConfig {
        server: ServerConfig {
            host: raw
                .get("host")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| defaults.server.host.clone()),
            port: raw
                .get("port")
                .and_then(|v| v.as_u64())
                .map(|v| v as u16)
                .unwrap_or(defaults.server.port),
        },
        storage: StorageConfig {
            database_path: raw
                .get("database_path")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| defaults.storage.database_path.clone()),
            model_directory: raw
                .get("model_directory")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| defaults.storage.model_directory.clone()),
            subtitle_history_limit: raw
                .get("subtitle_history_limit")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(defaults.storage.subtitle_history_limit),
        },
        audio: AudioConfig {
            sample_rate: raw
                .get("sample_rate")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
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
                    raw.get("audio_device_id").and_then(|v| v.as_i64())
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
        asr: serde_json::from_value(asr_raw).unwrap_or_default(),
        ..AppConfig::default()
    };
    fix_colliding_ports(&mut config);
    config
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

fn config_version(raw: &serde_json::Value) -> Result<u64, String> {
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
        v if v == SCHEMA_VERSION as u64 => {
            serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?
        }
        2 => {
            let mut config: AppConfig =
                serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
            fix_colliding_ports(&mut config);
            config
        }
        1 => migrate_v1(raw),
        other => return Err(format!("Unsupported configuration schema v{other}")),
    };
    config.schema_version = SCHEMA_VERSION;
    config.validate_settings()?;
    Ok(config)
}

pub fn load_config(path: &Path) -> Result<AppConfig, String> {
    if !path.exists() {
        let config = AppConfig::default();
        save_config(path, &config)?;
        return Ok(config);
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let raw: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("Configuration JSON is invalid: {error}"))?;
    let version = config_version(&raw)?;
    let config = config_from_value(&raw)?;
    if version != SCHEMA_VERSION as u64 {
        save_config(path, &config)?;
    }
    Ok(config)
}

/// 原子写入：先写临时文件再替换，避免崩溃时留下半截配置。
pub fn save_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let payload = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary: PathBuf =
        path.with_extension(format!("json.{}.{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(payload.as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let dir = std::env::temp_dir().join(format!("vrcs-config-{}", std::process::id()));
        let path = dir.join("config.json");
        let config = load_config(&path).unwrap();
        assert_eq!(config, AppConfig::default());
        let reloaded = load_config(&path).unwrap();
        assert_eq!(reloaded, config);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_v3_without_model_directory_uses_the_default() {
        let config = config_from_value(&serde_json::json!({
            "schema_version": 3
        }))
        .unwrap();

        assert_eq!(config.storage.model_directory, "models/whisper");
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
        assert_eq!(config.audio.output.device_id, Some(3));
        assert_eq!(config.audio.microphone.mode, "device");
        assert_eq!(config.audio.microphone.device_id, Some(7));
        assert_eq!(config.asr.model, "tiny");
        assert_eq!(config.asr.language, "ja");
        assert_eq!(config.asr.device, "auto");
    }

    #[test]
    fn migration_resolves_port_collision() {
        let raw = serde_json::json!({"port": 8765});
        let config = config_from_value(&raw).unwrap();
        assert_eq!(config.server.port, 8766);
    }

    #[test]
    fn rejects_invalid_vad() {
        let raw = serde_json::json!({
            "schema_version": 3,
            "vad": {"silence_seconds": 5.0}
        });
        assert!(config_from_value(&raw).is_err());
    }

    #[test]
    fn rejects_non_object_and_invalid_schema_version() {
        assert!(config_from_value(&serde_json::json!([])).is_err());
        assert!(config_from_value(&serde_json::json!({"schema_version": "3"})).is_err());
    }

    #[test]
    fn anki_name_limits_count_unicode_characters() {
        let mut config = AppConfig::default();
        config.anki.deck = "学".repeat(100);
        assert!(config.validate_settings().is_ok());
        config.anki.deck.push('ぶ');
        assert!(config.validate_settings().is_err());
    }
}
