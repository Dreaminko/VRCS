use serde::{Deserialize, Serialize};

use super::{ActiveApiProfiles, ApiProfile};

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

pub(super) fn default_asr_model() -> String {
    "small".into()
}

fn default_asr_backend() -> String {
    "qwen_realtime".into()
}

pub(super) fn default_language() -> String {
    "auto".into()
}

pub(super) fn default_device() -> String {
    "auto".into()
}

pub(super) fn default_compute_type() -> String {
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

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            backend: default_asr_backend(),
            language: default_language(),
            local: LocalAsrConfig::default(),
            qwen: QwenAsrConfig::default(),
            fun_asr: FunAsrConfig::default(),
            openai: OpenAiAsrConfig::default(),
            api_profiles: Vec::new(),
            active_api_profiles: ActiveApiProfiles::default(),
            cloud_failure_policy: default_cloud_failure_policy(),
        }
    }
}

impl Default for LocalAsrConfig {
    fn default() -> Self {
        Self {
            model: default_asr_model(),
            device: default_device(),
            compute_type: default_compute_type(),
        }
    }
}

impl Default for QwenAsrConfig {
    fn default() -> Self {
        Self {
            context: String::new(),
            model: default_qwen_model(),
        }
    }
}

impl Default for FunAsrConfig {
    fn default() -> Self {
        Self {
            context: String::new(),
            model: default_fun_asr_model(),
        }
    }
}

impl Default for OpenAiAsrConfig {
    fn default() -> Self {
        Self {
            model: default_openai_model(),
        }
    }
}
