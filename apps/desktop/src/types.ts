export type ConnectionState = "connecting" | "connected" | "disconnected";

export interface Subtitle {
  id: number | null;
  text: string;
  language: string | null;
  started_at: number | null;
  ended_at: number | null;
  created_at: string;
  source?: "speaker" | "microphone";
  translations: SubtitleTranslation[];
  translation_partial?: {
    text: string;
    target_language: string;
  };
}

export interface SubtitleTranslation {
  text: string;
  source_language: string | null;
  target_language: string;
  provider: ApiProvider | "local";
  model: string | null;
  created_at: string;
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
    context: string;
    model: "qwen3-asr-flash-realtime";
  };
  fun_asr: {
    context: string;
    model: "fun-asr-realtime";
  };
  openai: { model: "gpt-4o-mini-transcribe" | "gpt-4o-transcribe" };
  api_profiles: ApiProfile[];
  active_api_profiles: Record<AsrApiProvider, string | null>;
  cloud_failure_policy: "reconnect" | "local";
}

export type AsrApiProvider = "alibaba_cloud" | "openai";
export type ApiProvider = AsrApiProvider | "deepl" | "microsoft_translator";

export interface ApiProfile {
  id: string;
  name: string;
  provider: ApiProvider;
  region?: string;
  workspace_id?: string;
  base_url?: string;
}

export interface ApiProfileView extends ApiProfile {
  active: boolean;
  translation_active: boolean;
  credential: CredentialStatus;
}

export interface ApiModelCatalog {
  models: string[];
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

export interface CredentialStatus {
  configured: boolean;
  stored_configured: boolean;
  environment_override: boolean;
  source: "environment" | "credential_manager" | null;
}

export interface AudioOutputSettings {
  mode: "system" | "vrchat" | "disabled";
  device_id: number | null;
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

export interface AnkiSettings {
  enabled: boolean;
  port: number;
  deck: string;
  model: string;
  front_field: string;
  back_field: string;
}

export interface Settings {
  schema_version: 9;
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
  translation: TranslationSettings;
  osc: {
    enabled: boolean;
    port: number;
  };
  dictionary: {
    selection_lookup_enabled: boolean;
  };
  anki: AnkiSettings;
}

export interface TranslationSettings {
  mode: "disabled" | "manual" | "automatic";
  target_language: "zh-Hans" | "zh-Hant" | "en" | "ja" | "ko" | "es" | "fr" | "de" | "ru";
  profile_id: string | null;
  model: string;
  thinking_enabled: boolean;
  translate_microphone: boolean;
  microphone_target_language: "zh-Hans" | "zh-Hant" | "en" | "ja" | "ko" | "es" | "fr" | "de" | "ru";
}

export interface TranslationEvent {
  type: "translation_started" | "translation_partial" | "translation_completed" | "translation_failed";
  subtitle_id: number;
  text?: string;
  target_language?: string;
  translation?: SubtitleTranslation;
  code?: string;
  detail?: string;
}

export interface Health {
  status: string;
  config_schema?: number;
  capture_running: boolean;
  audio_device: AudioDevice | null;
  microphone_device?: AudioDevice | null;
  microphone_test_running?: boolean;
  microphone_test_device?: AudioDevice | null;
  asr_status: string;
  vad_backend: string;
  vad_model_version: string | null;
  last_error: string | null;
  osc?: OscRuntimeStatus;
}

export interface OscRuntimeStatus {
  enabled: boolean;
  target: string;
  status: "disabled" | "ready" | "error";
  last_error: string | null;
  last_sent_at: string | null;
  dropped_messages: number;
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
