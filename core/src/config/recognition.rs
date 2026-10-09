use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::providers::{
    SERVICE_FUN_ASR_REALTIME, SERVICE_GEMINI_TRANSCRIBE, SERVICE_GROQ_TRANSCRIPTION,
    SERVICE_OPENAI_REALTIME, SERVICE_QWEN_LOCAL_TRANSCRIPTION, SERVICE_QWEN_REALTIME,
    SERVICE_TOKEN_PLAN_REALTIME,
};

use super::ApiProfile;

pub const QWEN_MANAGED_BACKEND: &str = "qwen_local_managed";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsrConfig {
    #[serde(default = "default_asr_backend")]
    pub backend: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub managed_qwen: ManagedQwenConfig,
    #[serde(default)]
    pub api_profiles: Vec<ApiProfile>,
    #[serde(default)]
    pub active_profile_id: Option<String>,
    #[serde(default = "default_service_settings")]
    pub service_settings: BTreeMap<String, RecognitionServiceSettings>,
    /// Resolved per audio source; never persisted.
    #[serde(skip)]
    pub live_translation_target: Option<String>,
    /// Native translation glossary resolved from enabled sources; never persisted.
    #[serde(skip)]
    pub live_translation_phrases: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManagedQwenConfig {
    #[serde(default = "default_managed_qwen_package")]
    pub package_id: String,
    #[serde(default = "default_device")]
    pub device: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecognitionServiceSettings {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub context: String,
}

fn default_managed_qwen_package() -> String {
    "qwen3-asr-0.6b-q8_0".into()
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

pub fn default_service_settings() -> BTreeMap<String, RecognitionServiceSettings> {
    [
        (
            crate::providers::SERVICE_QWEN_LIVE_TRANSLATE,
            RecognitionServiceSettings {
                model: "qwen3.8-livetranslate-flash-realtime".into(),
                context: String::new(),
            },
        ),
        (
            crate::providers::SERVICE_OPENAI_REALTIME_TRANSLATE,
            RecognitionServiceSettings {
                model: "gpt-realtime-translate".into(),
                context: String::new(),
            },
        ),
        (
            crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE,
            RecognitionServiceSettings {
                model: "gemini-3.5-live-translate-preview".into(),
                context: String::new(),
            },
        ),
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
            SERVICE_TOKEN_PLAN_REALTIME,
            RecognitionServiceSettings {
                model: "qwen-audio-3.0-realtime-plus".into(),
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
        (
            SERVICE_QWEN_LOCAL_TRANSCRIPTION,
            RecognitionServiceSettings {
                model: "Qwen/Qwen3-ASR-0.6B".into(),
                context: String::new(),
            },
        ),
        (
            SERVICE_GEMINI_TRANSCRIBE,
            RecognitionServiceSettings {
                model: "gemini-3.5-transcribe-live".into(),
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
            managed_qwen: ManagedQwenConfig::default(),
            api_profiles: Vec::new(),
            active_profile_id: None,
            service_settings: default_service_settings(),
            live_translation_target: None,
            live_translation_phrases: BTreeMap::new(),
        }
    }
}

impl Default for ManagedQwenConfig {
    fn default() -> Self {
        Self {
            package_id: default_managed_qwen_package(),
            device: default_device(),
        }
    }
}
