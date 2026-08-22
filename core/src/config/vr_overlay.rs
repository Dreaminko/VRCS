use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VrOverlayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_translation_display")]
    pub translation_display: String,
    #[serde(default)]
    pub headset: VrOverlayHeadsetConfig,
    #[serde(default)]
    pub wrist: VrOverlayWristConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VrOverlayHeadsetConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_content_mode")]
    pub content_mode: String,
    #[serde(default)]
    pub show_partials: bool,
    #[serde(default)]
    pub show_translation_partials: bool,
    #[serde(default = "default_true")]
    pub include_speaker: bool,
    #[serde(default)]
    pub include_microphone: bool,
    #[serde(default)]
    pub include_chatbox: bool,
    #[serde(default)]
    pub offset_x_m: f32,
    #[serde(default = "default_headset_offset_y_m")]
    pub offset_y_m: f32,
    #[serde(default = "default_headset_distance_m")]
    pub distance_m: f32,
    #[serde(default = "default_headset_pitch_deg")]
    pub pitch_deg: f32,
    #[serde(default)]
    pub yaw_deg: f32,
    #[serde(default)]
    pub roll_deg: f32,
    #[serde(default = "default_headset_width_m")]
    pub width_m: f32,
    #[serde(default = "default_headset_opacity")]
    pub opacity: f32,
    #[serde(default = "default_headset_display_seconds")]
    pub display_seconds: f32,
    #[serde(default = "default_headset_fade_seconds")]
    pub fade_seconds: f32,
    #[serde(default = "default_headset_font_size_px")]
    pub font_size_px: u32,
    #[serde(default = "default_headset_background_opacity")]
    pub background_opacity: f32,
    #[serde(default)]
    pub vr_drag_edit_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VrOverlayWristConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_wrist_hand")]
    pub hand: String,
    #[serde(default = "default_dominant_hand")]
    pub dominant_hand: String,
    #[serde(default = "default_content_mode")]
    pub content_mode: String,
    #[serde(default)]
    pub show_partials: bool,
    #[serde(default)]
    pub show_translation_partials: bool,
    #[serde(default = "default_true")]
    pub include_speaker: bool,
    #[serde(default)]
    pub include_microphone: bool,
    #[serde(default)]
    pub include_chatbox: bool,
    #[serde(default = "default_wrist_max_entries")]
    pub max_entries: u32,
    #[serde(default)]
    pub idle_hide_seconds: u32,
    #[serde(default = "default_wrist_offset_x_m")]
    pub offset_x_m: f32,
    #[serde(default = "default_wrist_offset_y_m")]
    pub offset_y_m: f32,
    #[serde(default = "default_wrist_offset_z_m")]
    pub offset_z_m: f32,
    #[serde(default = "default_wrist_pitch_deg")]
    pub pitch_deg: f32,
    #[serde(default)]
    pub yaw_deg: f32,
    #[serde(default)]
    pub roll_deg: f32,
    #[serde(default = "default_wrist_width_m")]
    pub width_m: f32,
    #[serde(default = "default_wrist_opacity")]
    pub opacity: f32,
    #[serde(default = "default_wrist_font_size_px")]
    pub font_size_px: u32,
    #[serde(default = "default_wrist_background_opacity")]
    pub background_opacity: f32,
}

fn default_true() -> bool {
    true
}

fn default_content_mode() -> String {
    "bilingual".into()
}

fn default_translation_display() -> String {
    "all_languages".into()
}

fn default_headset_offset_y_m() -> f32 {
    -0.28
}

fn default_headset_distance_m() -> f32 {
    1.2
}

fn default_headset_pitch_deg() -> f32 {
    -8.0
}

fn default_headset_width_m() -> f32 {
    1.2
}

fn default_headset_opacity() -> f32 {
    0.92
}

fn default_headset_display_seconds() -> f32 {
    6.0
}

fn default_headset_fade_seconds() -> f32 {
    1.0
}

fn default_headset_font_size_px() -> u32 {
    54
}

fn default_headset_background_opacity() -> f32 {
    0.55
}

fn default_wrist_hand() -> String {
    "left".into()
}

fn default_dominant_hand() -> String {
    "right".into()
}

fn default_wrist_max_entries() -> u32 {
    5
}

fn default_wrist_offset_x_m() -> f32 {
    0.03
}

fn default_wrist_offset_y_m() -> f32 {
    0.08
}

fn default_wrist_offset_z_m() -> f32 {
    -0.06
}

fn default_wrist_pitch_deg() -> f32 {
    -55.0
}

fn default_wrist_width_m() -> f32 {
    0.32
}

fn default_wrist_opacity() -> f32 {
    0.94
}

fn default_wrist_font_size_px() -> u32 {
    32
}

fn default_wrist_background_opacity() -> f32 {
    0.65
}

impl Default for VrOverlayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            translation_display: default_translation_display(),
            headset: VrOverlayHeadsetConfig::default(),
            wrist: VrOverlayWristConfig::default(),
        }
    }
}

impl Default for VrOverlayHeadsetConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            content_mode: default_content_mode(),
            show_partials: false,
            show_translation_partials: false,
            include_speaker: default_true(),
            include_microphone: false,
            include_chatbox: false,
            offset_x_m: 0.0,
            offset_y_m: default_headset_offset_y_m(),
            distance_m: default_headset_distance_m(),
            pitch_deg: default_headset_pitch_deg(),
            yaw_deg: 0.0,
            roll_deg: 0.0,
            width_m: default_headset_width_m(),
            opacity: default_headset_opacity(),
            display_seconds: default_headset_display_seconds(),
            fade_seconds: default_headset_fade_seconds(),
            font_size_px: default_headset_font_size_px(),
            background_opacity: default_headset_background_opacity(),
            vr_drag_edit_enabled: false,
        }
    }
}

impl Default for VrOverlayWristConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            hand: default_wrist_hand(),
            dominant_hand: default_dominant_hand(),
            content_mode: default_content_mode(),
            show_partials: false,
            show_translation_partials: false,
            include_speaker: default_true(),
            include_microphone: false,
            include_chatbox: false,
            max_entries: default_wrist_max_entries(),
            idle_hide_seconds: 0,
            offset_x_m: default_wrist_offset_x_m(),
            offset_y_m: default_wrist_offset_y_m(),
            offset_z_m: default_wrist_offset_z_m(),
            pitch_deg: default_wrist_pitch_deg(),
            yaw_deg: 0.0,
            roll_deg: 0.0,
            width_m: default_wrist_width_m(),
            opacity: default_wrist_opacity(),
            font_size_px: default_wrist_font_size_px(),
            background_opacity: default_wrist_background_opacity(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognition_partials_can_be_enabled_explicitly() {
        let mut value = serde_json::to_value(VrOverlayConfig::default()).unwrap();
        value["headset"]["show_partials"] = serde_json::json!(true);
        value["wrist"]["show_partials"] = serde_json::json!(true);

        let config: VrOverlayConfig = serde_json::from_value(value).unwrap();

        assert!(config.headset.show_partials);
        assert!(config.wrist.show_partials);
    }

    #[test]
    fn translation_display_defaults_to_all_languages() {
        let config: VrOverlayConfig = serde_json::from_value(serde_json::json!({})).unwrap();

        assert_eq!(config.translation_display, "all_languages");
    }
}
