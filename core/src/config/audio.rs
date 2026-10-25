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
    pub pagefile_drive: String,
    pub opus_library_path: String,
    /// Zero disables the concurrent-player limit; individual audio queues remain bounded.
    pub max_players: usize,
    /// Visible player lanes only; zero shows all. Does not limit audio processing.
    pub max_visible_players: usize,
    /// None accepts every player; an empty list accepts none. Selection keys are
    /// stable VRChat usr_ IDs; track IDs remain independent runtime identities.
    pub selected_tracks: Option<Vec<String>>,
    /// Last known labels for the saved user-ID selection, for offline display only.
    pub selected_user_names: std::collections::BTreeMap<String, String>,
}

impl Default for PlayerAudioConfig {
    fn default() -> Self {
        Self {
            pagefile_drive: "C:".into(),
            opus_library_path: String::new(),
            max_players: 0,
            max_visible_players: 4,
            selected_tracks: None,
            selected_user_names: Default::default(),
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
