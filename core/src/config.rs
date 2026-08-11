//! 服务配置类型、默认值与结构校验。
//! 与 Python 版 `app/config.py` 行为保持一致。

use serde::{Deserialize, Serialize};

mod io;
mod migration;

pub use io::{load_config, save_config};
#[cfg(test)]
use migration::config_from_value;

pub const SCHEMA_VERSION: u32 = 9;

pub const ALIBABA_PROVIDER: &str = "alibaba_cloud";
pub const OPENAI_PROVIDER: &str = "openai";
pub const DEEPL_PROVIDER: &str = "deepl";
pub const MICROSOFT_PROVIDER: &str = "microsoft_translator";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiProfile {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl ApiProfile {
    pub fn uses_openai_compatible_api(&self) -> bool {
        self.provider == OPENAI_PROVIDER
            && self
                .base_url
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ActiveApiProfiles {
    #[serde(default)]
    pub alibaba_cloud: Option<String>,
    #[serde(default)]
    pub openai: Option<String>,
}

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
    #[serde(default = "default_microphone_trigger_threshold_dbfs")]
    pub trigger_threshold_dbfs: f32,
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
    #[serde(default)]
    pub api_profiles: Vec<ApiProfile>,
    #[serde(default)]
    pub active_api_profiles: ActiveApiProfiles,
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
    #[serde(default = "default_enabled")]
    pub enabled: bool,
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
pub struct DictionaryConfig {
    #[serde(default = "default_enabled")]
    pub selection_lookup_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationConfig {
    #[serde(default = "default_translation_mode")]
    pub mode: String,
    #[serde(default = "default_translation_target")]
    pub target_language: String,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default = "default_translation_model")]
    pub model: String,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default)]
    pub translate_microphone: bool,
    #[serde(default = "default_microphone_translation_target")]
    pub microphone_target_language: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OscConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_osc_port")]
    pub port: u16,
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
    pub dictionary: DictionaryConfig,
    #[serde(default)]
    pub translation: TranslationConfig,
    #[serde(default)]
    pub osc: OscConfig,
    #[serde(default)]
    pub anki: AnkiConfig,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}
fn default_host() -> String {
    "127.0.0.1".into()
}
fn default_enabled() -> bool {
    true
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
fn default_microphone_trigger_threshold_dbfs() -> f32 {
    -45.0
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
fn default_translation_mode() -> String {
    "disabled".into()
}
fn default_translation_target() -> String {
    "zh-Hans".into()
}
fn default_microphone_translation_target() -> String {
    "en".into()
}
fn default_translation_model() -> String {
    "gpt-5-mini".into()
}
fn default_osc_port() -> u16 {
    9000
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
impl_default!(MicrophoneConfig, {
    mode: default_microphone_mode,
    device_id: Option::default,
    trigger_threshold_dbfs: default_microphone_trigger_threshold_dbfs,
});
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
    api_profiles: Vec::default,
    active_api_profiles: ActiveApiProfiles::default,
    cloud_failure_policy: default_cloud_failure_policy,
});
impl_default!(LocalAsrConfig, {
    model: default_asr_model,
    device: default_device,
    compute_type: default_compute_type,
});
impl_default!(QwenAsrConfig, {
    context: String::default,
    model: default_qwen_model,
});
impl_default!(FunAsrConfig, {
    context: String::default,
    model: default_fun_asr_model,
});
impl_default!(OpenAiAsrConfig, { model: default_openai_model });
impl_default!(DictionaryConfig, { selection_lookup_enabled: default_enabled });
impl_default!(TranslationConfig, {
    mode: default_translation_mode,
    target_language: default_translation_target,
    profile_id: Option::default,
    model: default_translation_model,
    thinking_enabled: bool::default,
    translate_microphone: bool::default,
    microphone_target_language: default_microphone_translation_target,
});
impl_default!(OscConfig, { enabled: bool::default, port: default_osc_port });
impl_default!(AnkiConfig, {
    enabled: default_enabled,
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
            dictionary: DictionaryConfig::default(),
            translation: TranslationConfig::default(),
            osc: OscConfig::default(),
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
            return Err("Port must be between 1 and 65535".into());
        }
        if !(1..=10_000).contains(&self.storage.subtitle_history_limit) {
            return Err("subtitle_history_limit must be between 1 and 10000".into());
        }
        if self.storage.model_directory.trim().is_empty() {
            return Err("Model storage path cannot be empty".into());
        }
        if !(8_000..=96_000).contains(&self.audio.sample_rate) {
            return Err("Sample rate must be between 8000 and 96000".into());
        }
        match self.audio.output.mode.as_str() {
            "system" => {}
            "vrchat" | "disabled" if self.audio.output.device_id.is_some() => {
                return Err(
                    "VRChat or disabled output mode cannot specify a system output device".into(),
                );
            }
            "vrchat" | "disabled" => {}
            other => return Err(format!("Unsupported output mode: {other}")),
        }
        match self.audio.microphone.mode.as_str() {
            "device" if self.audio.microphone.device_id.is_none() => {
                return Err("A device must be selected in microphone device mode".into());
            }
            "device" => {}
            "default" | "disabled" if self.audio.microphone.device_id.is_some() => {
                return Err("Default or disabled microphone mode cannot specify a device".into());
            }
            "default" | "disabled" => {}
            other => return Err(format!("Unsupported microphone mode: {other}")),
        }
        if !(-80.0..=-10.0).contains(&self.audio.microphone.trigger_threshold_dbfs) {
            return Err("Microphone trigger_threshold_dbfs must be between -80 and -10".into());
        }
        self.vad.validate()?;
        if !ASR_BACKENDS.contains(&self.asr.backend.as_str()) {
            return Err(format!(
                "Unsupported recognition backend: {}",
                self.asr.backend
            ));
        }
        if !ASR_LANGUAGES.contains(&self.asr.language.as_str()) {
            return Err(format!(
                "Unsupported recognition language: {}",
                self.asr.language
            ));
        }
        if !ASR_DEVICES.contains(&self.asr.local.device.as_str()) {
            return Err(format!(
                "Unsupported recognition device: {}",
                self.asr.local.device
            ));
        }
        if !ASR_COMPUTE_TYPES.contains(&self.asr.local.compute_type.as_str()) {
            return Err(format!(
                "Unsupported compute type: {}",
                self.asr.local.compute_type
            ));
        }
        if !CLOUD_FAILURE_POLICIES.contains(&self.asr.cloud_failure_policy.as_str()) {
            return Err(format!(
                "Unsupported cloud failure policy: {}",
                self.asr.cloud_failure_policy
            ));
        }
        validate_api_profiles(&self.asr)?;
        validate_translation(&self.translation, &self.asr.api_profiles)?;
        if self.osc.port == 0 {
            return Err("OSC port must be between 1 and 65535".into());
        }
        if self.asr.qwen.model != "qwen3-asr-flash-realtime" {
            return Err(format!(
                "Unsupported Qwen ASR model: {}",
                self.asr.qwen.model
            ));
        }
        if self.asr.fun_asr.model != "fun-asr-realtime" {
            return Err(format!(
                "Unsupported Fun-ASR model: {}",
                self.asr.fun_asr.model
            ));
        }
        if self.asr.fun_asr.context.chars().count() > 400 {
            return Err("Fun-ASR context cannot exceed 400 characters".into());
        }
        if !["gpt-4o-mini-transcribe", "gpt-4o-transcribe"]
            .contains(&self.asr.openai.model.as_str())
        {
            return Err(format!(
                "Unsupported OpenAI ASR model: {}",
                self.asr.openai.model
            ));
        }
        if matches!(
            self.asr.backend.as_str(),
            "qwen_realtime" | "fun_asr_realtime"
        ) {
            if self.vad.silence_seconds < 0.2 {
                return Err(
                    "Alibaba Cloud realtime recognition requires at least 0.2 seconds of silence"
                        .into(),
                );
            }
        }
        if self.anki.port == 0 {
            return Err("AnkiConnect port must be between 1 and 65535".into());
        }
        for (label, value) in [
            ("deck", &self.anki.deck),
            ("note type", &self.anki.model),
            ("front field", &self.anki.front_field),
            ("back field", &self.anki.back_field),
        ] {
            if value.is_empty() || value.chars().count() > 100 {
                return Err(format!(
                    "Anki {label} name must contain 1 to 100 characters"
                ));
            }
        }
        if self.anki.front_field == self.anki.back_field {
            return Err("Anki front and back fields cannot map to the same field".into());
        }
        Ok(())
    }
}

fn validate_api_profiles(asr: &AsrConfig) -> Result<(), String> {
    use std::collections::HashSet;

    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for profile in &asr.api_profiles {
        let valid_id = !profile.id.is_empty()
            && profile.id.len() <= 64
            && profile
                .id
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'));
        if !valid_id || !ids.insert(profile.id.as_str()) {
            return Err("An API profile ID is invalid or duplicated".into());
        }
        let name = profile.name.trim();
        if name.is_empty() || name.chars().count() > 50 {
            return Err("An API profile name must contain 1 to 50 characters".into());
        }
        if !names.insert((profile.provider.as_str(), name.to_lowercase())) {
            return Err(format!(
                "API profile names must be unique per provider: {name}"
            ));
        }
        match profile.provider.as_str() {
            ALIBABA_PROVIDER => {
                if profile.base_url.is_some() {
                    return Err(
                        "Alibaba Cloud profiles cannot contain an OpenAI-compatible Base URL"
                            .into(),
                    );
                }
                let region = profile.region.as_deref().unwrap_or("");
                if !["singapore", "china_beijing"].contains(&region) {
                    return Err(format!("Unsupported Alibaba Cloud region: {region}"));
                }
                let workspace = profile.workspace_id.as_deref().unwrap_or("").trim();
                let valid_workspace = workspace
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || value == b'-');
                if !workspace.is_empty() && (workspace.len() > 128 || !valid_workspace) {
                    return Err("The Alibaba Cloud Workspace ID is invalid".into());
                }
            }
            MICROSOFT_PROVIDER => {
                let region = profile.region.as_deref().unwrap_or("").trim();
                if region.is_empty() || region.len() > 64 {
                    return Err(
                        "Microsoft Translator region must contain 1 to 64 characters".into(),
                    );
                }
                if profile.workspace_id.is_some() {
                    return Err(
                        "Microsoft Translator profiles cannot contain a Workspace ID".into(),
                    );
                }
                if profile.base_url.is_some() {
                    return Err(
                        "Microsoft Translator profiles cannot contain an OpenAI-compatible Base URL"
                            .into(),
                    );
                }
            }
            OPENAI_PROVIDER if profile.region.is_none() && profile.workspace_id.is_none() => {
                if let Some(base_url) = profile.base_url.as_deref() {
                    validate_openai_base_url(base_url)?;
                }
            }
            DEEPL_PROVIDER
                if profile.region.is_none()
                    && profile.workspace_id.is_none()
                    && profile.base_url.is_none() => {}
            OPENAI_PROVIDER | DEEPL_PROVIDER => {
                return Err(format!(
                    "API profile {} contains unsupported connection fields",
                    profile.provider
                ));
            }
            other => return Err(format!("Unsupported API provider: {other}")),
        }
    }

    for (provider, active_id) in [
        (
            ALIBABA_PROVIDER,
            asr.active_api_profiles.alibaba_cloud.as_deref(),
        ),
        (OPENAI_PROVIDER, asr.active_api_profiles.openai.as_deref()),
    ] {
        if let Some(active_id) = active_id {
            let active_profile = asr
                .api_profiles
                .iter()
                .find(|profile| profile.id == active_id && profile.provider == provider);
            if active_profile.is_none() {
                return Err(format!(
                    "The active API profile does not match provider {provider}"
                ));
            }
            if active_profile.is_some_and(ApiProfile::uses_openai_compatible_api) {
                return Err(
                    "OpenAI-compatible text API profiles cannot be used for realtime speech recognition"
                        .into(),
                );
            }
        }
    }
    Ok(())
}

fn validate_openai_base_url(base_url: &str) -> Result<(), String> {
    let base_url = base_url.trim();
    if base_url.is_empty() || base_url.len() > 2048 {
        return Err("The OpenAI-compatible Base URL must contain 1 to 2048 characters".into());
    }
    let url = reqwest::Url::parse(base_url)
        .map_err(|_| "The OpenAI-compatible Base URL is invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("The OpenAI-compatible Base URL must use HTTP or HTTPS".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "The OpenAI-compatible Base URL cannot contain credentials, a query, or a fragment"
                .into(),
        );
    }
    Ok(())
}

fn validate_translation(
    translation: &TranslationConfig,
    profiles: &[ApiProfile],
) -> Result<(), String> {
    if !["disabled", "manual", "automatic"].contains(&translation.mode.as_str()) {
        return Err(format!(
            "Unsupported translation mode: {}",
            translation.mode
        ));
    }
    const LANGUAGES: [&str; 9] = [
        "zh-Hans", "zh-Hant", "en", "ja", "ko", "es", "fr", "de", "ru",
    ];
    if !LANGUAGES.contains(&translation.target_language.as_str()) {
        return Err(format!(
            "Unsupported translation target language: {}",
            translation.target_language
        ));
    }
    if !LANGUAGES.contains(&translation.microphone_target_language.as_str()) {
        return Err(format!(
            "Unsupported microphone translation target language: {}",
            translation.microphone_target_language
        ));
    }
    if translation.mode == "disabled" && !translation.translate_microphone {
        return Ok(());
    }
    let profile_id = translation
        .profile_id
        .as_deref()
        .ok_or_else(|| "A translation API profile must be selected".to_string())?;
    let profile = profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "The selected translation API profile does not exist".to_string())?;
    if [ALIBABA_PROVIDER, OPENAI_PROVIDER].contains(&profile.provider.as_str())
        && translation.model.trim().is_empty()
    {
        return Err("The LLM translation model cannot be empty".into());
    }
    Ok(())
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
        });
        config.translation.profile_id = Some("openai-one".into());
        config.translation.model.clear();
        assert!(config.validate_settings().is_err());
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
            provider: OPENAI_PROVIDER.into(),
            region: None,
            workspace_id: None,
            base_url: Some("https://api.deepseek.com/v1".into()),
        });
        config.translation.mode = "manual".into();
        config.translation.profile_id = Some("deepseek".into());
        config.translation.model = "deepseek-chat".into();
        assert!(config.validate_settings().is_ok());

        config.asr.active_api_profiles.openai = Some("deepseek".into());
        assert_eq!(
            config.validate_settings().unwrap_err(),
            "OpenAI-compatible text API profiles cannot be used for realtime speech recognition"
        );

        config.asr.active_api_profiles.openai = None;
        config.asr.api_profiles[0].base_url =
            Some("https://api.deepseek.com/v1?token=secret".into());
        assert_eq!(
            config.validate_settings().unwrap_err(),
            "The OpenAI-compatible Base URL cannot contain credentials, a query, or a fragment"
        );
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
            },
            ApiProfile {
                id: "alibaba-two".into(),
                name: "personal".into(),
                provider: ALIBABA_PROVIDER.into(),
                region: Some("singapore".into()),
                workspace_id: Some("workspace-two".into()),
                base_url: None,
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
