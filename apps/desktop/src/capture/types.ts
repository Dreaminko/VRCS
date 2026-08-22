export interface AudioDevice {
  id: number;
  name: string;
  is_default: boolean;
  is_loopback: boolean;
  sample_rate: number;
  channels: number;
}

export interface LiveTranscription {
  type: "partial";
  utterance_id: string;
  source: "speaker" | "microphone";
  text: string;
  language?: string | null;
}

export interface AudioLevel {
  type: "audio_level";
  source: "speaker" | "microphone";
  rms_dbfs: number;
  peak_dbfs: number;
  speech: boolean;
}

export interface AudioOutputSettings {
  mode: "system" | "vrchat" | "disabled";
  device_id: number | null;
  trigger_threshold_dbfs: number;
}

export interface MicrophoneSettings {
  mode: "default" | "device" | "disabled";
  device_id: number | null;
  trigger_threshold_dbfs: number;
}

export interface VadSettings {
  silence_seconds: number;
  max_speech_seconds: number;
}
