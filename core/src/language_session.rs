use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, TranslationConfig, TranslationTargetConfig};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActiveLanguageSession {
    Global,
    Preset {
        preset_id: String,
        preset_name: String,
        #[serde(flatten)]
        snapshot: LanguageRuntimeConfig,
    },
    Override {
        #[serde(flatten)]
        snapshot: LanguageRuntimeConfig,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LanguageRuntimeConfig {
    pub recognition_language: String,
    pub translation: TranslationConfig,
    pub osc_translation_strategy: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureStartInput {
    #[serde(default)]
    pub language_preset_id: Option<String>,
    #[serde(default)]
    pub language_override: Option<LanguageOverrideInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageOverrideInput {
    pub recognition_language: String,
    pub translation_mode: String,
    pub speaker_targets: Vec<TranslationTargetConfig>,
    pub microphone_targets: Vec<TranslationTargetConfig>,
    pub osc_translation_strategy: String,
}

impl ActiveLanguageSession {
    pub fn resolve(&self, global: &AppConfig) -> LanguageRuntimeConfig {
        match self {
            Self::Global => LanguageRuntimeConfig::from_global(global),
            Self::Preset { snapshot, .. } | Self::Override { snapshot } => snapshot.clone(),
        }
    }

    pub fn apply_to(&self, global: &AppConfig) -> AppConfig {
        let mut effective = global.clone();
        let runtime = self.resolve(global);
        effective.asr.language = runtime.recognition_language;
        effective.translation = runtime.translation;
        effective.osc.translation_strategy = runtime.osc_translation_strategy;
        effective
    }
}

impl LanguageRuntimeConfig {
    fn from_global(config: &AppConfig) -> Self {
        Self {
            recognition_language: config.asr.language.clone(),
            translation: config.translation.clone(),
            osc_translation_strategy: config.osc.translation_strategy.clone(),
        }
    }
}

pub fn select_session(
    input: CaptureStartInput,
    global: &AppConfig,
) -> Result<ActiveLanguageSession, String> {
    match (input.language_preset_id, input.language_override) {
        (Some(_), Some(_)) => {
            Err("language_preset_id and language_override cannot be used together".into())
        }
        (Some(preset_id), None) => {
            let preset = global
                .language_presets
                .iter()
                .find(|preset| preset.id == preset_id)
                .ok_or_else(|| "The selected language preset does not exist".to_string())?;
            Ok(ActiveLanguageSession::Preset {
                preset_id: preset.id.clone(),
                preset_name: preset.name.clone(),
                snapshot: LanguageRuntimeConfig {
                    recognition_language: preset.recognition_language.clone(),
                    translation: TranslationConfig {
                        mode: preset.translation_mode.clone(),
                        speaker_targets: preset.speaker_targets.clone(),
                        microphone_targets: preset.microphone_targets.clone(),
                        prompt: global.translation.prompt.clone(),
                    },
                    osc_translation_strategy: preset.osc_translation_strategy.clone(),
                },
            })
        }
        (None, Some(override_config)) => {
            let snapshot = LanguageRuntimeConfig {
                recognition_language: override_config.recognition_language,
                translation: TranslationConfig {
                    mode: override_config.translation_mode,
                    speaker_targets: override_config.speaker_targets,
                    microphone_targets: override_config.microphone_targets,
                    prompt: global.translation.prompt.clone(),
                },
                osc_translation_strategy: override_config.osc_translation_strategy,
            };
            let mut candidate = global.clone();
            candidate.asr.language = snapshot.recognition_language.clone();
            candidate.translation = snapshot.translation.clone();
            candidate.osc.translation_strategy = snapshot.osc_translation_strategy.clone();
            candidate.validate_settings()?;
            Ok(ActiveLanguageSession::Override { snapshot })
        }
        (None, None) => Ok(ActiveLanguageSession::Global),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ambiguous_session_selection() {
        let result = select_session(
            CaptureStartInput {
                language_preset_id: Some("preset".into()),
                language_override: Some(LanguageOverrideInput {
                    recognition_language: "en".into(),
                    translation_mode: "disabled".into(),
                    speaker_targets: vec![TranslationTargetConfig::new("ja")],
                    microphone_targets: vec![TranslationTargetConfig::new("en")],
                    osc_translation_strategy: "preferred_only".into(),
                }),
            },
            &AppConfig::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn preset_sessions_keep_a_frozen_language_snapshot() {
        let mut global = AppConfig::default();
        global.language_presets.push(crate::config::LanguagePreset {
            id: "c7c4556a-dd7a-47d1-85f8-d4ce9bda3a96".into(),
            name: "Japanese".into(),
            recognition_language: "ja".into(),
            translation_mode: "disabled".into(),
            speaker_targets: vec![TranslationTargetConfig::new("en")],
            microphone_targets: vec![TranslationTargetConfig::new("ja")],
            osc_translation_strategy: "round_robin".into(),
        });
        let session = select_session(
            CaptureStartInput {
                language_preset_id: Some(global.language_presets[0].id.clone()),
                language_override: None,
            },
            &global,
        )
        .unwrap();
        global.asr.language = "de".into();
        global.translation.speaker_targets[0].target_language = "fr".into();

        let runtime = session.resolve(&global);
        assert_eq!(runtime.recognition_language, "ja");
        assert_eq!(runtime.translation.speaker_targets[0].target_language, "en");
        assert_eq!(runtime.osc_translation_strategy, "round_robin");
    }
}
