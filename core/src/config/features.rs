use serde::{Deserialize, Serialize};

use super::{AppConfig, GlossarySource};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeatureConfig {
    pub glossary: bool,
    pub learning: bool,
    pub anki: bool,
    pub osc_chatbox: bool,
    pub vrcx: bool,
    pub ocr: bool,
    pub vr_overlay: bool,
    pub external_api: bool,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            glossary: true,
            learning: true,
            anki: true,
            osc_chatbox: true,
            vrcx: true,
            ocr: true,
            vr_overlay: true,
            external_api: true,
        }
    }
}

#[derive(Clone, Copy)]
pub enum FeatureKey {
    Glossary,
    Learning,
    Anki,
    OscChatbox,
    Vrcx,
    Ocr,
    VrOverlay,
    ExternalApi,
}

impl FeatureKey {
    pub fn name(self) -> &'static str {
        match self {
            Self::Glossary => "glossary",
            Self::Learning => "learning",
            Self::Anki => "anki",
            Self::OscChatbox => "osc_chatbox",
            Self::Vrcx => "vrcx",
            Self::Ocr => "ocr",
            Self::VrOverlay => "vr_overlay",
            Self::ExternalApi => "external_api",
        }
    }

    pub fn enabled(self, features: &FeatureConfig) -> bool {
        match self {
            Self::Glossary => features.glossary,
            Self::Learning => features.learning,
            Self::Anki => features.anki,
            Self::OscChatbox => features.osc_chatbox,
            Self::Vrcx => features.vrcx,
            Self::Ocr => features.ocr,
            Self::VrOverlay => features.vr_overlay,
            Self::ExternalApi => features.external_api,
        }
    }
}

/// Apply module availability without changing the user's stored preferences.
pub fn apply_feature_gates(config: &AppConfig) -> AppConfig {
    let mut effective = config.clone();
    let features = &config.features;
    if !features.glossary {
        effective.glossary.asr_enabled = false;
        effective.glossary.llm_enabled = false;
        for source in &mut effective.glossary.sources {
            match source {
                GlossarySource::Local { enabled, .. }
                | GlossarySource::Subscription { enabled, .. } => *enabled = false,
            }
        }
    }
    effective.dictionary.selection_lookup_enabled &= features.learning;
    effective.anki.enabled &= features.anki;
    effective.osc.enabled &= features.osc_chatbox;
    effective.osc.mute_sync_enabled &= features.osc_chatbox;
    effective.osc.mute_status_toast_enabled &= features.osc_chatbox;
    effective.vrcx.enabled &= features.vrcx;
    effective.external_api.enabled &= features.external_api;
    effective.vr_overlay.enabled &= features.vr_overlay;
    effective.ocr.enabled &= features.ocr && features.vr_overlay;
    effective.ocr.desktop_enabled &= features.ocr;
    effective
}
