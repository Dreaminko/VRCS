use serde::{Deserialize, Serialize};

use super::{
    AppConfig, ReasoningSetting, TranslationBackend, TranslationConfig, TranslationTargetConfig,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelBinding {
    #[serde(default)]
    pub backend: TranslationBackend,
    pub profile_id: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelSelection {
    #[serde(default)]
    pub use_default_model: bool,
    #[serde(default)]
    pub backend: TranslationBackend,
    pub profile_id: Option<String>,
    pub model: String,
}

impl Default for ModelSelection {
    fn default() -> Self {
        Self {
            use_default_model: true,
            backend: TranslationBackend::Api,
            profile_id: None,
            model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelDefaultsConfig {
    pub text: ModelBinding,
    pub translation: ModelBinding,
    pub vision: ModelBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LearningAiConfig {
    #[serde(default)]
    pub use_default_model: bool,
    pub profile_id: Option<String>,
    pub model: String,
    pub thinking_enabled: bool,
    pub reasoning_control: Option<ReasoningSetting>,
}

impl Default for LearningAiConfig {
    fn default() -> Self {
        Self {
            use_default_model: true,
            profile_id: None,
            model: String::new(),
            thinking_enabled: false,
            reasoning_control: None,
        }
    }
}

impl LearningAiConfig {
    pub(crate) fn resolve_model_assignment(&mut self, default: &ModelBinding) {
        if self.use_default_model && inherit_api(&mut self.profile_id, &mut self.model, default) {
            self.reasoning_control = None;
        }
        if self.use_default_model && default.backend == TranslationBackend::PlamoLocal {
            self.thinking_enabled = false;
            self.reasoning_control = None;
        }
    }
}

fn inherit_api(
    profile_id: &mut Option<String>,
    model: &mut String,
    binding: &ModelBinding,
) -> bool {
    let changed = *profile_id != binding.profile_id || *model != binding.model;
    profile_id.clone_from(&binding.profile_id);
    model.clone_from(&binding.model);
    changed
}

fn inherit(
    profile_id: &mut Option<String>,
    model: &mut String,
    backend: &mut TranslationBackend,
    binding: &ModelBinding,
) -> bool {
    let changed =
        *profile_id != binding.profile_id || *model != binding.model || *backend != binding.backend;
    profile_id.clone_from(&binding.profile_id);
    model.clone_from(&binding.model);
    *backend = binding.backend;
    changed
}

impl ModelSelection {
    fn resolve(&mut self, default: &ModelBinding) -> ModelBinding {
        if self.use_default_model {
            inherit(
                &mut self.profile_id,
                &mut self.model,
                &mut self.backend,
                default,
            );
        }
        ModelBinding {
            backend: self.backend,
            profile_id: self.profile_id.clone(),
            model: self.model.clone(),
        }
    }
}

fn resolve_targets(targets: &mut [TranslationTargetConfig], binding: &ModelBinding) {
    let has_shared_binding = binding.backend == TranslationBackend::PlamoLocal
        || binding.profile_id.is_some()
        || !binding.model.trim().is_empty();
    for target in targets {
        target.use_default_model = true;
        // Preserve legacy route copies until the user configures a shared task model.
        if has_shared_binding
            && inherit(
                &mut target.profile_id,
                &mut target.model,
                &mut target.backend,
                binding,
            )
        {
            target.reasoning_control = None;
        }
        if target.backend == TranslationBackend::PlamoLocal {
            target.thinking_enabled = false;
            target.reasoning_control = None;
        }
    }
}

fn clear_changed_target_controls(
    targets: &mut [TranslationTargetConfig],
    previous: &[TranslationTargetConfig],
) {
    for target in targets {
        if target.use_default_model
            && previous.iter().any(|old| {
                old.target_language == target.target_language
                    && (old.backend != target.backend
                        || old.profile_id != target.profile_id
                        || old.model != target.model)
            })
        {
            target.reasoning_control = None;
            target.thinking_enabled = false;
        }
    }
}

impl TranslationConfig {
    pub(crate) fn resolve_model_assignments(&mut self, default: &ModelBinding) {
        let binding = self.model_selection.resolve(default);
        resolve_targets(&mut self.speaker_targets, &binding);
        resolve_targets(&mut self.microphone_targets, &binding);
    }
}

impl AppConfig {
    pub(crate) fn clear_changed_inherited_reasoning(&mut self, previous: &Self) {
        clear_changed_target_controls(
            &mut self.translation.speaker_targets,
            &previous.translation.speaker_targets,
        );
        clear_changed_target_controls(
            &mut self.translation.microphone_targets,
            &previous.translation.microphone_targets,
        );
        clear_changed_target_controls(&mut self.ocr.targets, &previous.ocr.targets);
        for preset in &mut self.language_presets {
            if let Some(old) = previous
                .language_presets
                .iter()
                .find(|old| old.id == preset.id)
            {
                clear_changed_target_controls(&mut preset.speaker_targets, &old.speaker_targets);
                clear_changed_target_controls(
                    &mut preset.microphone_targets,
                    &old.microphone_targets,
                );
            }
        }
        if self.vision.use_default_model
            && (self.vision.profile_id != previous.vision.profile_id
                || self.vision.model != previous.vision.model)
        {
            self.vision.reasoning_control = None;
        }
        if self.learning_ai.use_default_model
            && (self.learning_ai.profile_id != previous.learning_ai.profile_id
                || self.learning_ai.model != previous.learning_ai.model)
        {
            self.learning_ai.reasoning_control = None;
        }
    }

    /// Refresh effective pairs without changing inheritance or feature switches.
    pub fn resolve_model_assignments(&mut self) {
        let translation_default = &self.model_defaults.translation;
        self.translation
            .resolve_model_assignments(translation_default);
        let ocr = self.ocr.model_selection.resolve(translation_default);
        resolve_targets(&mut self.ocr.targets, &ocr);
        let binding = ModelBinding {
            backend: self.translation.model_selection.backend,
            profile_id: self.translation.model_selection.profile_id.clone(),
            model: self.translation.model_selection.model.clone(),
        };
        for preset in &mut self.language_presets {
            resolve_targets(&mut preset.speaker_targets, &binding);
            resolve_targets(&mut preset.microphone_targets, &binding);
        }
        let titles = &mut self.conversation_titles;
        if titles.use_default_model {
            inherit_api(
                &mut titles.profile_id,
                &mut titles.model,
                &self.model_defaults.text,
            );
        }
        let vision = &mut self.vision;
        if vision.use_default_model
            && inherit_api(
                &mut vision.profile_id,
                &mut vision.model,
                &self.model_defaults.vision,
            )
        {
            vision.reasoning_control = None;
        }
        self.learning_ai
            .resolve_model_assignment(&self.model_defaults.text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_session::{select_session, CaptureStartInput};

    #[test]
    fn model_assignment_refreshes_followers_and_language_snapshots_only() {
        let mut config = AppConfig::default();
        config.model_defaults.text = ModelBinding {
            backend: TranslationBackend::Api,
            profile_id: Some("shared".into()),
            model: "text-one".into(),
        };
        config.model_defaults.translation = ModelBinding {
            backend: TranslationBackend::Api,
            profile_id: Some("shared".into()),
            model: "translation-one".into(),
        };
        config.model_defaults.vision = ModelBinding {
            backend: TranslationBackend::Api,
            profile_id: Some("images".into()),
            model: "vision-one".into(),
        };
        let legacy_override = &mut config.translation.microphone_targets[0];
        legacy_override.use_default_model = false;
        legacy_override.profile_id = Some("custom".into());
        legacy_override.model = "custom-model".into();
        legacy_override
            .model_by_profile
            .insert("custom".into(), "remembered".into());
        config.ocr.model_selection = ModelSelection {
            use_default_model: false,
            backend: TranslationBackend::Api,
            profile_id: Some("ocr".into()),
            model: "ocr-model".into(),
        };
        config.translation.mode = "automatic".into();
        config.asr.backend = crate::providers::SERVICE_QWEN_LIVE_TRANSLATE.into();
        config.language_presets.push(super::super::LanguagePreset {
            id: "preset".into(),
            name: "Preset".into(),
            recognition_language: "ja".into(),
            translation_mode: "automatic".into(),
            speaker_targets: config.translation.speaker_targets.clone(),
            microphone_targets: config.translation.microphone_targets.clone(),
            osc_translation_strategy: "preferred_only".into(),
        });
        config.resolve_model_assignments();
        let session = select_session(
            CaptureStartInput {
                language_preset_id: Some("preset".into()),
                language_override: None,
            },
            &config,
        )
        .unwrap();
        config.learning_ai.thinking_enabled = true;
        config.learning_ai.reasoning_control = Some(ReasoningSetting::Effort("high".into()));
        config.translation.speaker_targets[0].reasoning_control =
            config.learning_ai.reasoning_control.clone();
        let previous = config.clone();
        config.model_defaults.text.model = "text-two".into();
        config.resolve_model_assignments();
        assert_eq!(
            config.translation.speaker_targets[0].model,
            "translation-one"
        );
        config.model_defaults.translation.model = "translation-two".into();
        config.resolve_model_assignments();
        assert_eq!(
            config.translation.speaker_targets[0].model,
            "translation-two"
        );
        assert_eq!(
            config.translation.microphone_targets[0].model,
            "translation-two"
        );
        assert!(config.translation.microphone_targets[0].use_default_model);
        assert_eq!(config.ocr.targets[0].model, "ocr-model");
        assert_eq!(config.conversation_titles.model, "text-two");
        assert_eq!(config.vision.model, "vision-one");
        assert_eq!(config.learning_ai.model, "text-two");
        assert!(config.learning_ai.use_default_model && config.learning_ai.thinking_enabled);
        assert!(config.learning_ai.reasoning_control.is_none());
        assert!(config.translation.speaker_targets[0]
            .reasoning_control
            .is_none());
        assert_eq!(config.translation.mode, "automatic");
        assert_eq!(config.features, previous.features);
        assert_eq!(config.asr, previous.asr);
        config.learning_ai.reasoning_control = previous.learning_ai.reasoning_control.clone();
        config.clear_changed_inherited_reasoning(&previous);
        assert!(config.learning_ai.reasoning_control.is_none());
        config.translation.model_selection = ModelSelection {
            use_default_model: false,
            backend: TranslationBackend::Api,
            profile_id: Some("task".into()),
            model: "task-model".into(),
        };
        config.resolve_model_assignments();
        let runtime = session.apply_to(&config);
        assert_eq!(runtime.translation.speaker_targets[0].model, "task-model");
        assert_eq!(
            runtime.translation.microphone_targets[0].model,
            "task-model"
        );
    }

    #[test]
    fn local_translation_inheritance_clears_thinking_controls() {
        let mut config = AppConfig::default();
        config.translation.model_selection = ModelSelection {
            use_default_model: false,
            backend: TranslationBackend::PlamoLocal,
            profile_id: None,
            model: crate::translation::MODEL_ID.into(),
        };
        let target = &mut config.translation.speaker_targets[0];
        target.use_default_model = true;
        target.thinking_enabled = true;
        target.reasoning_control = Some(ReasoningSetting::Effort("high".into()));

        config.resolve_model_assignments();

        let target = &config.translation.speaker_targets[0];
        assert_eq!(target.backend, TranslationBackend::PlamoLocal);
        assert!(!target.thinking_enabled);
        assert!(target.reasoning_control.is_none());
    }

    #[test]
    fn model_assignment_migrates_legacy_bindings_and_round_trips_resolved_defaults() {
        let defaults = AppConfig::default();
        assert!(defaults.translation.model_selection.use_default_model);
        assert!(defaults.translation.speaker_targets[0].use_default_model);
        assert!(defaults.ocr.targets[0].use_default_model);
        assert!(defaults.conversation_titles.use_default_model);
        assert!(defaults.vision.use_default_model && defaults.learning_ai.use_default_model);
        let mut config = super::super::migration::config_from_value(&serde_json::json!({
            "schema_version": 37,
            "asr": {"api_profiles": [{"id":"shared", "name":"Shared", "provider":"openai_compatible",
                "base_url":"http://127.0.0.1:9000/v1", "auth_mode":"none",
                "enabled_capabilities":["text_generation", "text_translation", "image_understanding"]}]},
            "translation": {"speaker_targets":[{"target_language":"ja", "profile_id":"shared", "model":"legacy-text"}]},
            "ocr": {"targets":[{"target_language":"en", "profile_id":"shared", "model":"legacy-ocr"}]},
            "conversation_titles": {"profile_id":"shared", "model":"legacy-titles"},
            "vision": {"profile_id":"shared", "model":"legacy-vision"},
            "language_presets":[{"id":"c7c4556a-dd7a-47d1-85f8-d4ce9bda3a96", "name":"Preset", "recognition_language":"ja", "translation_mode":"disabled",
                "speaker_targets":[{"target_language":"en", "profile_id":"shared", "model":"legacy-preset"}],
                "microphone_targets":[{"target_language":"ja"}], "osc_translation_strategy":"preferred_only"}]
        })).unwrap();
        assert!(config.translation.speaker_targets[0].use_default_model);
        assert!(config.ocr.targets[0].use_default_model);
        assert!(!config.conversation_titles.use_default_model);
        assert!(!config.vision.use_default_model);
        assert!(config.language_presets[0].speaker_targets[0].use_default_model);
        config.model_defaults.text = ModelBinding {
            backend: TranslationBackend::Api,
            profile_id: Some("shared".into()),
            model: "new-text".into(),
        };
        config.model_defaults.translation = ModelBinding {
            backend: TranslationBackend::Api,
            profile_id: Some("shared".into()),
            model: "new-translation".into(),
        };
        config.model_defaults.vision = ModelBinding {
            backend: TranslationBackend::Api,
            profile_id: Some("shared".into()),
            model: "new-vision".into(),
        };
        config.resolve_model_assignments();
        assert_eq!(
            config.translation.speaker_targets[0].model,
            "new-translation"
        );
        assert_eq!(
            config.translation.microphone_targets[0].model,
            "new-translation"
        );
        assert_eq!(config.ocr.targets[0].model, "new-translation");
        assert_eq!(
            config.language_presets[0].speaker_targets[0].model,
            "new-translation"
        );
        assert_eq!(config.learning_ai.model, "new-text");
        assert_eq!(config.vision.model, "legacy-vision");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        super::super::save_config(&path, &config).unwrap();
        assert_eq!(super::super::load_config(&path).unwrap(), config);
        config.validate_settings().unwrap();
        let raw = serde_json::to_value(config).unwrap();
        assert_eq!(raw["schema_version"], 41);
        assert_eq!(
            raw["learning_ai"]["reasoning_control"],
            serde_json::Value::Null
        );
    }
}
