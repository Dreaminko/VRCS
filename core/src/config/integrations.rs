use serde::{Deserialize, Serialize};

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
pub struct OscConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_osc_port")]
    pub port: u16,
    #[serde(default = "default_enabled")]
    pub mute_sync_enabled: bool,
    #[serde(default)]
    pub mute_status_toast_enabled: bool,
    #[serde(default = "default_enabled")]
    pub preserve_original_text: bool,
    #[serde(default = "super::language::default_translation_strategy")]
    pub translation_strategy: String,
}

fn default_enabled() -> bool {
    true
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

impl Default for AnkiConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            port: default_anki_port(),
            deck: default_anki_deck(),
            model: default_anki_model(),
            front_field: default_front_field(),
            back_field: default_back_field(),
        }
    }
}

impl Default for DictionaryConfig {
    fn default() -> Self {
        Self {
            selection_lookup_enabled: default_enabled(),
        }
    }
}

impl Default for OscConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_osc_port(),
            mute_sync_enabled: default_enabled(),
            mute_status_toast_enabled: false,
            preserve_original_text: default_enabled(),
            translation_strategy: super::language::default_translation_strategy(),
        }
    }
}
