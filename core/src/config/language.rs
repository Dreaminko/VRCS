use serde::{Deserialize, Serialize};

use super::TranslationTargetConfig;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanguagePreset {
    pub id: String,
    pub name: String,
    pub recognition_language: String,
    pub translation_mode: String,
    pub speaker_targets: Vec<TranslationTargetConfig>,
    pub microphone_targets: Vec<TranslationTargetConfig>,
    #[serde(default = "default_translation_strategy")]
    pub osc_translation_strategy: String,
}

pub(crate) fn default_translation_strategy() -> String {
    "preferred_only".into()
}
