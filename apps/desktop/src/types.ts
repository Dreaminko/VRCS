export type ConnectionState = "connecting" | "connected" | "disconnected";

export interface Subtitle {
  id: number | null;
  text: string;
  language: string | null;
  started_at: number | null;
  ended_at: number | null;
  created_at: string;
  source?: "speaker" | "microphone";
}

export interface AudioDevice {
  id: number;
  name: string;
  is_default: boolean;
  is_loopback: boolean;
  sample_rate: number;
  channels: number;
}

export interface AsrSettings {
  model: "tiny" | "base" | "small" | "medium" | "large-v3";
  language: "auto" | "en" | "ja" | "zh" | "ko" | "es" | "fr" | "de";
  device: "auto" | "cpu" | "cuda";
  compute_type: "int8" | "float16" | "int8_float16";
}

export interface Settings {
  host: string;
  port: number;
  database_path: string;
  audio_device_id: number | null;
  microphone_device_id: number | null;
  sample_rate: number;
  subtitle_history_limit: number;
  asr: AsrSettings;
}

export interface Health {
  status: string;
  capture_running: boolean;
  audio_device: AudioDevice | null;
  microphone_device?: AudioDevice | null;
  asr_status: string;
  vad_backend: string;
  last_error: string | null;
}

export interface DictionaryEntry {
  term: string;
  language: string;
  definition: string;
  reading?: string | null;
  dictionary?: string | null;
}

export interface DictionarySource {
  id: number;
  title: string;
  revision: string;
  source_language: string;
  target_language: string | null;
  entry_count: number;
  imported_at: string;
}
