use serde::{Deserialize, Serialize};

use super::{
    AnkiConfig, AsrConfig, AudioConfig, DictionaryConfig, ExternalApiConfig, OscConfig,
    ServerConfig, StorageConfig, TranslationConfig, VadConfig,
};

pub const SCHEMA_VERSION: u32 = 19;

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
    #[serde(default)]
    pub external_api: ExternalApiConfig,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

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
            external_api: ExternalApiConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlossaryCategory, GlossaryEntry, GlossarySource, TranslationPromptConfig};

    #[test]
    fn default_config_json_contract_is_stable() {
        let value = serde_json::to_value(AppConfig::default()).unwrap();
        assert_keys(
            &value,
            [
                "anki",
                "asr",
                "audio",
                "dictionary",
                "external_api",
                "osc",
                "schema_version",
                "server",
                "storage",
                "translation",
                "vad",
            ],
        );
        assert_keys(&value["server"], ["host", "port"]);
        assert_keys(
            &value["storage"],
            [
                "database_path",
                "model_directory",
                "subtitle_history_max_bytes",
            ],
        );
        assert_keys(&value["audio"], ["microphone", "output", "sample_rate"]);
        assert_keys(&value["audio"]["output"], ["device_id", "mode"]);
        assert_keys(
            &value["audio"]["microphone"],
            ["device_id", "mode", "trigger_threshold_dbfs"],
        );
        assert_keys(&value["vad"], ["max_speech_seconds", "silence_seconds"]);
        assert_keys(
            &value["asr"],
            [
                "active_api_profiles",
                "api_profiles",
                "backend",
                "cloud_failure_policy",
                "fun_asr",
                "language",
                "local",
                "openai",
                "qwen",
            ],
        );
        assert_keys(
            &value["asr"]["active_api_profiles"],
            ["alibaba_cloud", "openai"],
        );
        assert_keys(&value["asr"]["local"], ["compute_type", "device", "model"]);
        assert_keys(&value["asr"]["qwen"], ["context", "model"]);
        assert_keys(&value["asr"]["fun_asr"], ["context", "model"]);
        assert_keys(&value["asr"]["openai"], ["model"]);
        assert_keys(&value["dictionary"], ["selection_lookup_enabled"]);
        assert_keys(
            &value["translation"],
            [
                "microphone_target_language",
                "mode",
                "model",
                "profile_id",
                "prompt",
                "target_language",
                "thinking_enabled",
                "translate_microphone",
            ],
        );
        assert_keys(
            &value["translation"]["prompt"],
            [
                "context_enabled",
                "glossary_sources",
                "include_chatbox",
                "include_microphone",
                "include_speaker",
                "max_chars",
                "max_messages",
                "system_prompt",
            ],
        );
        assert_keys(
            &value["osc"],
            [
                "enabled",
                "mute_status_toast_enabled",
                "mute_sync_enabled",
                "port",
            ],
        );
        assert_keys(
            &value["anki"],
            [
                "back_field",
                "deck",
                "enabled",
                "front_field",
                "model",
                "port",
            ],
        );
        assert_keys(
            &value["external_api"],
            ["enabled", "host", "port", "require_token"],
        );
        assert_eq!(value["schema_version"], SCHEMA_VERSION);

        let round_trip: AppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip, AppConfig::default());
    }

    #[test]
    fn glossary_sources_use_the_tagged_json_contract() {
        let mut prompt = TranslationPromptConfig::default();
        prompt.glossary_sources = vec![
            GlossarySource::Local {
                id: "local-one".into(),
                name: "Names".into(),
                enabled: true,
                entries: vec![GlossaryEntry {
                    source: "VRChat".into(),
                    target: None,
                    category: GlossaryCategory::Game,
                    case_sensitive: false,
                }],
            },
            GlossarySource::Subscription {
                id: "remote-one".into(),
                url: "https://example.com/glossary.json".into(),
                display_name: None,
                enabled: false,
            },
        ];
        prompt.glossary = vec![GlossaryEntry {
            source: "runtime-only".into(),
            target: None,
            category: GlossaryCategory::Custom,
            case_sensitive: false,
        }];

        let value = serde_json::to_value(&prompt).unwrap();

        assert!(value.get("glossary").is_none());
        assert_eq!(
            value["glossary_sources"],
            serde_json::json!([
                {
                    "id": "local-one",
                    "type": "local",
                    "name": "Names",
                    "enabled": true,
                    "entries": [{
                        "source": "VRChat",
                        "target": null,
                        "category": "game",
                        "case_sensitive": false
                    }]
                },
                {
                    "id": "remote-one",
                    "type": "subscription",
                    "url": "https://example.com/glossary.json",
                    "display_name": null,
                    "enabled": false
                }
            ])
        );
        let deserialized: TranslationPromptConfig = serde_json::from_value(serde_json::json!({
            "glossary": [{"source": "ignored"}],
            "glossary_sources": value["glossary_sources"].clone()
        }))
        .unwrap();
        assert!(deserialized.glossary.is_empty());
        assert_eq!(deserialized.glossary_sources, prompt.glossary_sources);
    }

    fn assert_keys<const N: usize>(value: &serde_json::Value, expected: [&str; N]) {
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(keys, expected);
    }
}
