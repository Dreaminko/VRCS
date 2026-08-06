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
  backend: "local_whisper" | "qwen_realtime" | "fun_asr_realtime" | "openai_realtime";
  language: "auto" | "en" | "ja" | "zh" | "ko" | "es" | "fr" | "de";
  local: {
    model: "tiny" | "base" | "small" | "medium" | "large-v3";
    device: "auto" | "cpu" | "cuda";
    compute_type: "int8";
  };
  qwen: {
    region: "singapore" | "china_beijing";
    workspace_id: string;
    context: string;
    model: "qwen3-asr-flash-realtime";
  };
  fun_asr: {
    context: string;
    model: "fun-asr-realtime";
  };
  openai: { model: "gpt-4o-mini-transcribe" | "gpt-4o-transcribe" };
  cloud_failure_policy: "reconnect" | "local";
}

export interface LiveTranscription {
  type: "partial";
  utterance_id: string;
  source: "speaker" | "microphone";
  text: string;
  language?: string | null;
}

export interface CredentialStatus {
  configured: boolean;
  source: "environment" | "credential_manager" | null;
}

export interface AudioOutputSettings {
  mode: "system" | "vrchat";
  device_id: number | null;
}

export interface MicrophoneSettings {
  mode: "default" | "device" | "disabled";
  device_id: number | null;
}

export interface VadSettings {
  silence_seconds: number;
  max_speech_seconds: number;
}

export interface AnkiSettings {
  enabled: boolean;
  port: number;
  deck: string;
  model: string;
  front_field: string;
  back_field: string;
}

export interface Settings {
  schema_version: 5;
  server: {
    host: string;
    port: number;
  };
  storage: {
    database_path: string;
    model_directory: string;
    subtitle_history_limit: number;
  };
  audio: {
    sample_rate: number;
    output: AudioOutputSettings;
    microphone: MicrophoneSettings;
  };
  vad: VadSettings;
  asr: AsrSettings;
  dictionary: {
    selection_lookup_enabled: boolean;
  };
  anki: AnkiSettings;
}

export interface Health {
  status: string;
  config_schema?: number;
  capture_running: boolean;
  audio_device: AudioDevice | null;
  microphone_device?: AudioDevice | null;
  asr_status: string;
  vad_backend: string;
  vad_model_version: string | null;
  last_error: string | null;
}

export interface AsrModelCapability {
  id: AsrSettings["local"]["model"];
  repository: string;
  status: "not_downloaded" | "downloaded" | "loading" | "ready" | "error";
}

export type AsrModelStatus =
  | "not_downloaded"
  | "downloading"
  | "downloaded"
  | "loading"
  | "ready"
  | "error";

export interface AsrModelRecord {
  id: AsrSettings["local"]["model"];
  repository: string;
  status: AsrModelStatus;
  active: boolean;
  downloaded_bytes: number;
  total_bytes: number;
  progress: number;
  error: string | null;
}

export interface AsrCapabilities {
  runtime_available: boolean;
  cuda: {
    available: boolean;
    device_count: number;
    error: string | null;
  };
  compute_types: Record<AsrSettings["local"]["device"], AsrSettings["local"]["compute_type"][]>;
  models: AsrModelCapability[];
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

export interface DictionaryImportProgress {
  progress: number;
}

export interface AnkiStatus {
  connected: boolean;
  version: number | null;
  decks: string[];
  models: string[];
  fields: string[];
  configuration_valid: boolean;
  error_code: string | null;
  status_code: string;
  params: Record<string, unknown>;
  detail: string;
  message: string;
}

export interface AnkiCardInput {
  term: string;
  definition: string;
  context: string;
  reading?: string | null;
  dictionary?: string | null;
  language?: string | null;
  labels?: {
    definition: string;
    context: string;
  };
}
