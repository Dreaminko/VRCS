//! 服务配置类型、默认值与结构校验。
//! 与 Python 版 `app/config.py` 行为保持一致。

use serde::{Deserialize, Serialize};

mod io;
mod migration;

pub use io::{load_config, save_config};
#[cfg(test)]
use migration::config_from_value;

pub const SCHEMA_VERSION: u32 = 4;

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
    #[serde(default = "default_asr_backend")]
    pub backend: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub local: LocalAsrConfig,
    #[serde(default)]
    pub qwen: QwenAsrConfig,
    #[serde(default)]
    pub fun_asr: FunAsrConfig,
    #[serde(default)]
    pub openai: OpenAiAsrConfig,
    #[serde(default = "default_cloud_failure_policy")]
    pub cloud_failure_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalAsrConfig {
    #[serde(default = "default_asr_model")]
    pub model: String,
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_compute_type")]
    pub compute_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QwenAsrConfig {
    #[serde(default = "default_qwen_region")]
    pub region: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub context: String,
    #[serde(default = "default_qwen_model")]
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunAsrConfig {
    #[serde(default)]
    pub context: String,
    #[serde(default = "default_fun_asr_model")]
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiAsrConfig {
    #[serde(default = "default_openai_model")]
    pub model: String,
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
fn default_asr_backend() -> String {
    "qwen_realtime".into()
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
fn default_qwen_region() -> String {
    "china_beijing".into()
}
fn default_qwen_model() -> String {
    "qwen3-asr-flash-realtime".into()
}
fn default_openai_model() -> String {
    "gpt-4o-mini-transcribe".into()
}
fn default_fun_asr_model() -> String {
    "fun-asr-realtime".into()
}
fn default_cloud_failure_policy() -> String {
    "reconnect".into()
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
    backend: default_asr_backend,
    language: default_language,
    local: LocalAsrConfig::default,
    qwen: QwenAsrConfig::default,
    fun_asr: FunAsrConfig::default,
    openai: OpenAiAsrConfig::default,
    cloud_failure_policy: default_cloud_failure_policy,
});
impl_default!(LocalAsrConfig, {
    model: default_asr_model,
    device: default_device,
    compute_type: default_compute_type,
});
impl_default!(QwenAsrConfig, {
    region: default_qwen_region,
    workspace_id: String::default,
    context: String::default,
    model: default_qwen_model,
});
impl_default!(FunAsrConfig, {
    context: String::default,
    model: default_fun_asr_model,
});
impl_default!(OpenAiAsrConfig, { model: default_openai_model });
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
const ASR_BACKENDS: [&str; 4] = [
    "local_whisper",
    "qwen_realtime",
    "fun_asr_realtime",
    "openai_realtime",
];
const ASR_DEVICES: [&str; 3] = ["auto", "cpu", "cuda"];
const ASR_COMPUTE_TYPES: [&str; 1] = ["int8"];
const CLOUD_FAILURE_POLICIES: [&str; 2] = ["reconnect", "local"];

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
        if !ASR_BACKENDS.contains(&self.asr.backend.as_str()) {
            return Err(format!("不支持的识别后端：{}", self.asr.backend));
        }
        if !ASR_LANGUAGES.contains(&self.asr.language.as_str()) {
            return Err(format!("不支持的识别语言：{}", self.asr.language));
        }
        if !ASR_DEVICES.contains(&self.asr.local.device.as_str()) {
            return Err(format!("不支持的识别设备：{}", self.asr.local.device));
        }
        if !ASR_COMPUTE_TYPES.contains(&self.asr.local.compute_type.as_str()) {
            return Err(format!("不支持的计算类型：{}", self.asr.local.compute_type));
        }
        if !CLOUD_FAILURE_POLICIES.contains(&self.asr.cloud_failure_policy.as_str()) {
            return Err(format!(
                "不支持的云端失败策略：{}",
                self.asr.cloud_failure_policy
            ));
        }
        if !["singapore", "china_beijing"].contains(&self.asr.qwen.region.as_str()) {
            return Err(format!("不支持的 Qwen 区域：{}", self.asr.qwen.region));
        }
        if self.asr.qwen.model != "qwen3-asr-flash-realtime" {
            return Err(format!("不支持的 Qwen ASR 模型：{}", self.asr.qwen.model));
        }
        if self.asr.fun_asr.model != "fun-asr-realtime" {
            return Err(format!("不支持的 Fun-ASR 模型：{}", self.asr.fun_asr.model));
        }
        if self.asr.fun_asr.context.chars().count() > 400 {
            return Err("Fun-ASR 上下文不能超过 400 个字符".into());
        }
        if !["gpt-4o-mini-transcribe", "gpt-4o-transcribe"]
            .contains(&self.asr.openai.model.as_str())
        {
            return Err(format!(
                "不支持的 OpenAI ASR 模型：{}",
                self.asr.openai.model
            ));
        }
        if matches!(
            self.asr.backend.as_str(),
            "qwen_realtime" | "fun_asr_realtime"
        ) {
            let workspace = self.asr.qwen.workspace_id.trim();
            let valid_workspace = workspace
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || value == b'-');
            if !workspace.is_empty() && (workspace.len() > 128 || !valid_workspace) {
                return Err("阿里云 Workspace ID 无效".into());
            }
            if self.vad.silence_seconds < 0.2 {
                return Err("阿里云实时识别的静音阈值不能低于 0.2 秒".into());
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        assert_eq!(config.schema_version, 4);
        assert_eq!(config.asr.backend, "local_whisper");
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
    fn rejects_invalid_vad() {
        let raw = serde_json::json!({
            "schema_version": 4,
            "vad": {"silence_seconds": 5.0}
        });
        assert!(config_from_value(&raw).is_err());
    }

    #[test]
    fn qwen_is_the_default_backend() {
        let config = AppConfig::default();
        assert_eq!(config.asr.backend, "qwen_realtime");
        assert_eq!(config.asr.qwen.region, "china_beijing");
        assert!(config.asr.qwen.workspace_id.is_empty());
        assert!(config.validate_settings().is_ok());
    }

    #[test]
    fn validates_fun_asr_specific_limits() {
        let mut config = AppConfig::default();
        config.asr.backend = "fun_asr_realtime".into();
        config.asr.qwen.workspace_id = "ws-example".into();
        config.asr.fun_asr.context = "字".repeat(400);
        assert!(config.validate_settings().is_ok());

        config.asr.fun_asr.context.push('字');
        assert_eq!(
            config.validate_settings().unwrap_err(),
            "Fun-ASR 上下文不能超过 400 个字符"
        );
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
