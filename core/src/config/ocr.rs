use serde::{Deserialize, Serialize};

pub const DEFAULT_VISION_SYSTEM_PROMPT: &str = "Answer the user's question about the image concisely. Use the requested answer language. If the image does not provide enough information, say so. Do not invent details.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VisionConfig {
    pub enabled: bool,
    pub vr_enabled: bool,
    pub desktop_enabled: bool,
    pub desktop_shortcut: String,
    pub asr: Option<VisionAsrConfig>,
    pub profile_id: Option<String>,
    pub model: String,
    pub thinking_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_control: Option<super::ReasoningSetting>,
    pub answer_language: String,
    pub system_prompt: String,
    pub max_capture_seconds: u32,
    pub asr_finalize_timeout_ms: u64,
    pub request_timeout_seconds: u32,
    pub max_output_tokens: u32,
    pub image_max_edge: u32,
    pub image_max_bytes: usize,
    pub display_seconds: f32,
    pub wrist: Option<VrOcrWristConfig>,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            vr_enabled: true,
            desktop_enabled: false,
            desktop_shortcut: "Ctrl+Alt+V".into(),
            asr: None,
            profile_id: None,
            model: String::new(),
            thinking_enabled: false,
            reasoning_control: None,
            answer_language: "zh-Hans".into(),
            system_prompt: DEFAULT_VISION_SYSTEM_PROMPT.into(),
            max_capture_seconds: 30,
            asr_finalize_timeout_ms: 10_000,
            request_timeout_seconds: 60,
            max_output_tokens: 1024,
            image_max_edge: 1536,
            image_max_bytes: 4_194_304,
            display_seconds: 30.0,
            wrist: Some(VrOcrWristConfig::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VisionAsrConfig {
    pub backend: String,
    pub profile_id: Option<String>,
    pub language: String,
    pub model: String,
}

impl Default for VisionAsrConfig {
    fn default() -> Self {
        Self {
            backend: super::QWEN_MANAGED_BACKEND.into(),
            profile_id: None,
            language: "auto".into(),
            model: String::new(),
        }
    }
}

impl VisionConfig {
    pub fn asr_config(&self, shared: &super::AsrConfig) -> super::AsrConfig {
        let Some(custom) = &self.asr else {
            return shared.clone();
        };
        let mut config = super::AsrConfig {
            backend: custom.backend.clone(),
            active_profile_id: custom.profile_id.clone(),
            language: custom.language.clone(),
            api_profiles: shared
                .api_profiles
                .iter()
                .filter(|profile| Some(&profile.id) == custom.profile_id.as_ref())
                .cloned()
                .collect(),
            service_settings: Default::default(),
            ..Default::default()
        };
        if custom.backend == super::QWEN_MANAGED_BACKEND {
            config.managed_qwen = shared.managed_qwen.clone();
        } else {
            config.service_settings.insert(
                custom.backend.clone(),
                super::RecognitionServiceSettings {
                    model: custom.model.clone(),
                    context: String::new(),
                },
            );
        }
        config
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VrOcrConfig {
    pub master_enabled: bool,
    pub enabled: bool,
    pub desktop_enabled: bool,
    pub shortcut: String,
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
            master_enabled: true,
            enabled: false,
            desktop_enabled: false,
            shortcut: "Ctrl+Alt+O".into(),
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
