use serde::{Deserialize, Serialize};

use super::{
    AnkiConfig, AsrConfig, AudioConfig, DictionaryConfig, ExternalApiConfig, GlossaryConfig,
    LanguagePreset, OscConfig, ServerConfig, StorageConfig, TranslationConfig, VadConfig,
    VrOverlayConfig, VrcxConfig,
};

pub const SCHEMA_VERSION: u32 = 26;

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
    pub glossary: GlossaryConfig,
    #[serde(default)]
    pub translation: TranslationConfig,
    #[serde(default)]
    pub language_presets: Vec<LanguagePreset>,
    #[serde(default)]
    pub osc: OscConfig,
    #[serde(default)]
    pub anki: AnkiConfig,
    #[serde(default)]
    pub external_api: ExternalApiConfig,
    #[serde(default)]
    pub vrcx: VrcxConfig,
    #[serde(default)]
    pub vr_overlay: VrOverlayConfig,
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
            glossary: GlossaryConfig::default(),
            translation: TranslationConfig::default(),
            language_presets: Vec::new(),
            osc: OscConfig::default(),
            anki: AnkiConfig::default(),
            external_api: ExternalApiConfig::default(),
            vrcx: VrcxConfig::default(),
            vr_overlay: VrOverlayConfig::default(),
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
                "glossary",
                "language_presets",
                "osc",
                "schema_version",
                "server",
                "storage",
                "translation",
                "vad",
                "vr_overlay",
                "vrcx",
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
        assert_keys(
            &value["audio"]["output"],
            ["device_id", "mode", "trigger_threshold_dbfs"],
        );
        assert_keys(
            &value["audio"]["microphone"],
            ["device_id", "mode", "trigger_threshold_dbfs"],
        );
        assert_keys(&value["vad"], ["max_speech_seconds", "silence_seconds"]);
        assert_keys(
            &value["asr"],
            [
                "active_profile_id",
                "api_profiles",
                "backend",
                "cloud_failure_policy",
                "language",
                "local",
                "service_settings",
            ],
        );
        assert_keys(&value["asr"]["local"], ["compute_type", "device", "model"]);
        assert_keys(
            &value["asr"]["service_settings"]["qwen_realtime"],
            ["context", "model"],
        );
        assert_keys(
            &value["asr"]["service_settings"]["fun_asr_realtime"],
            ["context", "model"],
        );
        assert_keys(
            &value["asr"]["service_settings"]["openai_realtime"],
            ["context", "model"],
        );
        assert_keys(
            &value["asr"]["service_settings"]["groq_transcription"],
            ["context", "model"],
        );
        assert_keys(&value["dictionary"], ["selection_lookup_enabled"]);
        assert_keys(
            &value["glossary"],
            ["asr_enabled", "llm_enabled", "sources"],
        );
        assert_keys(
            &value["translation"],
            ["microphone_targets", "mode", "prompt", "speaker_targets"],
        );
        assert_keys(
            &value["translation"]["speaker_targets"][0],
            ["model", "profile_id", "target_language", "thinking_enabled"],
        );
        assert_keys(
            &value["translation"]["prompt"],
            [
                "context_enabled",
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
                "preserve_original_text",
                "translation_strategy",
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
        assert_keys(&value["vr_overlay"], ["enabled", "headset", "wrist"]);
        assert_keys(
            &value["vr_overlay"]["headset"],
            [
                "background_opacity",
                "content_mode",
                "display_seconds",
                "distance_m",
                "enabled",
                "fade_seconds",
                "font_size_px",
                "include_chatbox",
                "include_microphone",
                "include_speaker",
                "offset_x_m",
                "offset_y_m",
                "opacity",
                "pitch_deg",
                "roll_deg",
                "show_partials",
                "show_translation_partials",
                "vr_drag_edit_enabled",
                "width_m",
                "yaw_deg",
            ],
        );
        assert_keys(
            &value["vr_overlay"]["wrist"],
            [
                "background_opacity",
                "content_mode",
                "dominant_hand",
                "enabled",
                "font_size_px",
                "hand",
                "idle_hide_seconds",
                "include_chatbox",
                "include_microphone",
                "include_speaker",
                "max_entries",
                "offset_x_m",
                "offset_y_m",
                "offset_z_m",
                "opacity",
                "pitch_deg",
                "roll_deg",
                "show_partials",
                "show_translation_partials",
                "width_m",
                "yaw_deg",
            ],
        );
        assert_keys(
            &value["vrcx"],
            [
                "enabled",
                "include_in_asr_context",
                "include_in_llm_context",
                "port",
            ],
        );
        assert_eq!(value["schema_version"], SCHEMA_VERSION);

        let round_trip: AppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip, AppConfig::default());
    }

    #[test]
    fn vr_overlay_nested_defaults_are_complete_and_unknown_fields_are_rejected() {
        let config: VrOverlayConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "headset": {"enabled": false},
            "wrist": {"hand": "right"}
        }))
        .unwrap();

        assert!(config.enabled);
        assert!(!config.headset.enabled);
        assert_eq!(config.headset.distance_m, 1.2);
        assert_eq!(config.wrist.hand, "right");
        assert_eq!(config.wrist.max_entries, 5);

        assert!(
            serde_json::from_value::<VrOverlayConfig>(serde_json::json!({
                "headset": {"unknown": true}
            }))
            .is_err()
        );
    }

    #[test]
    fn glossary_consumer_switches_default_to_enabled_when_omitted() {
        let glossary: GlossaryConfig = serde_json::from_value(serde_json::json!({
            "sources": []
        }))
        .unwrap();

        assert!(glossary.llm_enabled);
        assert!(glossary.asr_enabled);
    }

    #[test]
    fn glossary_sources_use_the_tagged_json_contract() {
        let glossary = GlossaryConfig {
            sources: vec![
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
            ],
            ..Default::default()
        };

        let value = serde_json::to_value(&glossary).unwrap();

        assert_eq!(
            value["sources"],
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
        assert_eq!(
            serde_json::from_value::<GlossaryConfig>(value).unwrap(),
            glossary
        );

        let prompt = TranslationPromptConfig {
            glossary: vec![GlossaryEntry {
                source: "runtime-only".into(),
                target: None,
                category: GlossaryCategory::Custom,
                case_sensitive: false,
            }],
            ..Default::default()
        };
        assert!(serde_json::to_value(prompt)
            .unwrap()
            .get("glossary")
            .is_none());
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
