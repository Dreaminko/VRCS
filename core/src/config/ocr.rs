use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VrOcrConfig {
    pub enabled: bool,
    pub desktop_enabled: bool,
    pub shortcut: String,
    pub backend: VrOcrBackend,
    pub device: OcrDevice,
    pub display_mode: VrOcrDisplayMode,
    pub timeout_seconds: u32,
    pub minimum_confidence: f32,
    pub region_fraction: f32,
    pub targets: Vec<super::TranslationTargetConfig>,
    pub hand_gesture_enabled: bool,
    pub display_seconds: f32,
    pub background_opacity: f32,
    pub wrist: Option<VrOcrWristConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VrOcrWristConfig {
    pub hand: String,
    pub dominant_hand: String,
    pub offset_x_m: f32,
    pub offset_y_m: f32,
    pub offset_z_m: f32,
    pub pitch_deg: f32,
    pub yaw_deg: f32,
    pub roll_deg: f32,
    pub width_m: f32,
    pub font_size_px: u32,
    pub opacity: f32,
}

impl From<&super::VrOverlayWristConfig> for VrOcrWristConfig {
    fn from(wrist: &super::VrOverlayWristConfig) -> Self {
        Self {
            hand: wrist.hand.clone(),
            dominant_hand: wrist.dominant_hand.clone(),
            offset_x_m: wrist.offset_x_m,
            offset_y_m: wrist.offset_y_m,
            offset_z_m: wrist.offset_z_m,
            pitch_deg: wrist.pitch_deg,
            yaw_deg: wrist.yaw_deg,
            roll_deg: wrist.roll_deg,
            width_m: wrist.width_m,
            font_size_px: 32,
            opacity: wrist.opacity,
        }
    }
}

impl Default for VrOcrWristConfig {
    fn default() -> Self {
        Self::from(&super::VrOverlayWristConfig::default())
    }
}

impl VrOcrWristConfig {
    pub fn overlay_config(&self) -> super::VrOverlayWristConfig {
        super::VrOverlayWristConfig {
            hand: self.hand.clone(),
            dominant_hand: self.dominant_hand.clone(),
            offset_x_m: self.offset_x_m,
            offset_y_m: self.offset_y_m,
            offset_z_m: self.offset_z_m,
            pitch_deg: self.pitch_deg,
            yaw_deg: self.yaw_deg,
            roll_deg: self.roll_deg,
            width_m: self.width_m,
            font_size_px: self.font_size_px,
            opacity: self.opacity,
            ..Default::default()
        }
    }
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
pub enum OcrDevice {
    #[default]
    Cpu,
    Directml,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VrOcrDisplayMode {
    Wrist,
    #[default]
    Stereo,
}

impl Default for VrOcrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            desktop_enabled: false,
            shortcut: "Ctrl+Alt+O".into(),
            backend: VrOcrBackend::Cloud,
            device: OcrDevice::Cpu,
            display_mode: VrOcrDisplayMode::Stereo,
            timeout_seconds: 30,
            minimum_confidence: 0.6,
            region_fraction: 0.6,
            targets: vec![super::TranslationTargetConfig::new("zh-Hans")],
            hand_gesture_enabled: true,
            display_seconds: 15.0,
            background_opacity: 1.0,
            wrist: None,
        }
    }
}
