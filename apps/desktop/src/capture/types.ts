export interface AudioDevice {
  id: number;
  name: string;
  is_default: boolean;
  is_loopback: boolean;
  sample_rate: number;
  channels: number;
}

export interface LiveTranscription {
  speaker?: import("../subtitles/types").SpeakerIdentity | null;
  type: "partial";
  utterance_id: string;
  source_utterance_id?: string | null;
  completed_original?: string | null;
  conversation_preview?: { text: string; translation: string } | null;
  source: "speaker" | "microphone";
  text: string;
  language?: string | null;
  translation?: string;
  target_language?: string;
}

export interface AudioLevel {
  type: "audio_level";
  source: "speaker" | "microphone";
  rms_dbfs: number;
  peak_dbfs: number;
  speech: boolean;
}

export interface AudioOutputSettings {
  mode: "system" | "vrchat" | "vrchat_players" | "disabled";
  device_id: number | null;
  trigger_threshold_dbfs: number;
  player_audio?: PlayerAudioSettings;
}

export interface PlayerAudioSettings {
  pagefile_drive: string;
  opus_library_path: string;
  max_players: number;
  max_visible_players?: number;
  /** Persisted VRChat user IDs; null accepts everyone, [] accepts no one. */
  selected_tracks?: string[] | null;
  selected_user_names?: Record<string, string>;
}

export interface MicrophoneSettings {
  mode: "default" | "device" | "disabled";
  device_id: number | null;
  trigger_threshold_dbfs: number;
}

export interface VadSettings {
  endpointing: "silence" | "smart_turn";
  silence_seconds: number;
  max_speech_seconds: number;
}
