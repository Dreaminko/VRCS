use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VrOcrConfig {
    pub enabled: bool,
    pub desktop_enabled: bool,
    pub shortcut: String,
    pub backend: VrOcrBackend,
    pub display_mode: VrOcrDisplayMode,
    pub timeout_seconds: u32,
    pub minimum_confidence: f32,
    pub region_fraction: f32,
    pub targets: Vec<super::TranslationTargetConfig>,
    pub hand_gesture_enabled: bool,
    pub display_seconds: f32,
    pub background_opacity: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VrOcrBackend {
    #[default]
    Cloud,
    Local,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VrOcrDisplayMode {
    #[default]
    Wrist,
    Stereo,
}

impl Default for VrOcrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            desktop_enabled: false,
            shortcut: "Ctrl+Alt+O".into(),
            backend: VrOcrBackend::Cloud,
            display_mode: VrOcrDisplayMode::Wrist,
            timeout_seconds: 30,
            minimum_confidence: 0.6,
            region_fraction: 0.6,
            targets: vec![super::TranslationTargetConfig::new("zh-Hans")],
            hand_gesture_enabled: true,
            display_seconds: 15.0,
            background_opacity: 0.75,
        }
    }
}
