use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_output_mode")]
    pub mode: String,
    #[serde(default)]
    pub device_id: Option<i64>,
    #[serde(default = "default_output_trigger_threshold_dbfs")]
    pub trigger_threshold_dbfs: f32,
    #[serde(default)]
    pub player_audio: PlayerAudioConfig,
}

/// Opt-in parameters for the experimental Windows network audio source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerAudioConfig {
    pub enabled: bool,
    pub pagefile_drive: String,
    pub opus_library_path: String,
    /// Zero disables the concurrent-player limit; individual audio queues remain bounded.
    pub max_players: usize,
    /// Visible player lanes only; zero shows all. Does not limit audio processing.
    pub max_visible_players: usize,
    /// Retired per-player allow list. Configuration loading clears it so an upgrade
    /// cannot silently restrict processing; use `source_overrides` instead.
    pub selected_tracks: Option<Vec<String>>,
    /// Last known labels for the saved user-ID selection, for offline display only.
    pub selected_user_names: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub proximity: PlayerProximityConfig,
    /// Rank players in the local horizontal view ahead of others when resources compete.
    #[serde(default)]
    pub view_priority: PlayerViewPriorityConfig,
    /// Per-player source allow list keyed by stable VRChat usr_ IDs.
    #[serde(default)]
    pub source_overrides: std::collections::BTreeMap<String, PlayerSourceOverride>,
    /// Player admission for accounts without an explicit rule: `allowlist` keeps
    /// both sources off until enabled, `everyone` keeps both on until disabled.
    #[serde(default = "default_source_mode")]
    pub source_mode: String,
}

pub(crate) fn default_source_mode() -> String {
    "allowlist".into()
}

/// A player's enabled inputs. Both sources stay off until a player is enabled;
/// a missing entry denies both, and `false` marks the allowed source.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerSourceOverride {
    pub chatbox_disabled: bool,
    pub voice_disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerProximityConfig {
    pub enabled: bool,
    pub radius_m: f32,
}

impl Default for PlayerProximityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            radius_m: 10.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerViewPriorityConfig {
    pub enabled: bool,
    pub horizontal_fov_deg: f32,
}

impl Default for PlayerViewPriorityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            horizontal_fov_deg: 120.0,
        }
    }
}

impl Default for PlayerAudioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            pagefile_drive: "C:".into(),
            opus_library_path: String::new(),
            max_players: 0,
            max_visible_players: 4,
            selected_tracks: None,
            selected_user_names: Default::default(),
            proximity: PlayerProximityConfig::default(),
            view_priority: PlayerViewPriorityConfig::default(),
            source_overrides: Default::default(),
            source_mode: default_source_mode(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicrophoneConfig {
    #[serde(default = "default_microphone_mode")]
    pub mode: String,
    #[serde(default)]
    pub device_id: Option<i64>,
    #[serde(default = "default_microphone_trigger_threshold_dbfs")]
    pub trigger_threshold_dbfs: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioConfig {
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub microphone: MicrophoneConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VadConfig {
    #[serde(default = "default_endpointing")]
    pub endpointing: String,
    #[serde(default = "default_silence_seconds")]
    pub silence_seconds: f64,
    #[serde(default = "default_max_speech_seconds")]
    pub max_speech_seconds: f64,
}

fn default_output_mode() -> String {
    "system".into()
}

fn default_microphone_mode() -> String {
    "disabled".into()
}

fn default_output_trigger_threshold_dbfs() -> f32 {
    -45.0
}

fn default_microphone_trigger_threshold_dbfs() -> f32 {
    -45.0
}

fn default_sample_rate() -> u32 {
    16_000
}

fn default_endpointing() -> String {
    "silence".into()
}

fn default_silence_seconds() -> f64 {
    0.4
}

fn default_max_speech_seconds() -> f64 {
    6.0
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            mode: default_output_mode(),
            device_id: None,
            trigger_threshold_dbfs: default_output_trigger_threshold_dbfs(),
            player_audio: PlayerAudioConfig::default(),
        }
    }
}

impl Default for MicrophoneConfig {
    fn default() -> Self {
        Self {
            mode: default_microphone_mode(),
            device_id: None,
            trigger_threshold_dbfs: default_microphone_trigger_threshold_dbfs(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: default_sample_rate(),
            output: OutputConfig::default(),
            microphone: MicrophoneConfig::default(),
        }
    }
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            endpointing: default_endpointing(),
            silence_seconds: default_silence_seconds(),
            max_speech_seconds: default_max_speech_seconds(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_capture_master_defaults_off_and_accepts_legacy_parameters() {
        let defaults = serde_json::to_value(PlayerAudioConfig::default()).unwrap();
        assert_eq!(defaults["enabled"], false);
        assert_eq!(
            defaults["proximity"],
            serde_json::json!({"enabled": false, "radius_m": 10.0})
        );
        assert_eq!(
            defaults["view_priority"],
            serde_json::json!({"enabled": false, "horizontal_fov_deg": 120.0})
        );

        let legacy = serde_json::json!({
            "pagefile_drive": "D:",
            "opus_library_path": "custom-opus.dll",
            "max_players": 3,
            "max_visible_players": 2,
            "selected_tracks": [],
            "selected_user_names": {}
        });
        let config: PlayerAudioConfig = serde_json::from_value(legacy.clone()).unwrap();
        let mut saved = serde_json::to_value(config).unwrap();
        assert_eq!(
            saved.as_object_mut().unwrap().remove("enabled"),
            Some(false.into())
        );
        assert_eq!(
            saved.as_object_mut().unwrap().remove("proximity"),
            Some(serde_json::json!({"enabled": false, "radius_m": 10.0}))
        );
        assert_eq!(
            saved.as_object_mut().unwrap().remove("view_priority"),
            Some(serde_json::json!({"enabled": false, "horizontal_fov_deg": 120.0}))
        );
        assert_eq!(
            saved.as_object_mut().unwrap().remove("source_overrides"),
            Some(serde_json::json!({}))
        );
        assert_eq!(
            saved.as_object_mut().unwrap().remove("source_mode"),
            Some("allowlist".into())
        );
        assert_eq!(saved, legacy);
    }

    #[test]
    fn network_capture_master_persists_explicit_opt_in_and_rejects_non_boolean() {
        let mut saved = serde_json::to_value(PlayerAudioConfig::default()).unwrap();
        saved["enabled"] = true.into();
        let config: PlayerAudioConfig = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(serde_json::to_value(config).unwrap(), saved);
        saved["enabled"] = "true".into();
        assert!(serde_json::from_value::<PlayerAudioConfig>(saved).is_err());
    }
}
