use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::providers::{
    SERVICE_FUN_ASR_REALTIME, SERVICE_GROQ_TRANSCRIPTION, SERVICE_OPENAI_REALTIME,
    SERVICE_QWEN_REALTIME,
};

use super::ApiProfile;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsrConfig {
    #[serde(default = "default_asr_backend")]
    pub backend: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub local: LocalAsrConfig,
    #[serde(default)]
    pub api_profiles: Vec<ApiProfile>,
    #[serde(default)]
    pub active_profile_id: Option<String>,
    #[serde(default = "default_service_settings")]
    pub service_settings: BTreeMap<String, RecognitionServiceSettings>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecognitionServiceSettings {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub context: String,
}

pub(super) fn default_asr_model() -> String {
    "small".into()
}

fn default_asr_backend() -> String {
    SERVICE_QWEN_REALTIME.into()
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

fn default_cloud_failure_policy() -> String {
    "reconnect".into()
}

pub fn default_service_settings() -> BTreeMap<String, RecognitionServiceSettings> {
    [
        (
            SERVICE_QWEN_REALTIME,
            RecognitionServiceSettings {
                model: "qwen3-asr-flash-realtime".into(),
                context: String::new(),
            },
        ),
        (
            SERVICE_FUN_ASR_REALTIME,
            RecognitionServiceSettings {
                model: "fun-asr-realtime".into(),
                context: String::new(),
            },
        ),
        (
            SERVICE_OPENAI_REALTIME,
            RecognitionServiceSettings {
                model: "gpt-4o-mini-transcribe".into(),
                context: String::new(),
            },
        ),
        (
            SERVICE_GROQ_TRANSCRIPTION,
            RecognitionServiceSettings {
                model: "whisper-large-v3-turbo".into(),
                context: String::new(),
            },
        ),
    ]
    .into_iter()
    .map(|(service, settings)| (service.to_string(), settings))
    .collect()
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            backend: default_asr_backend(),
            language: default_language(),
            local: LocalAsrConfig::default(),
            api_profiles: Vec::new(),
            active_profile_id: None,
            service_settings: default_service_settings(),
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
