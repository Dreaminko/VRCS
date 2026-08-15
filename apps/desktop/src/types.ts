export type ConnectionState = "connecting" | "connected" | "disconnected";

export interface Subtitle {
  id: number | null;
  text: string;
  language: string | null;
  started_at: number | null;
  ended_at: number | null;
  created_at: string;
  source?: "speaker" | "microphone" | "chatbox";
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

export type ChatboxSendMode = "original" | "translation" | "bilingual";
export type ChatboxMessageFormat =
  | "original_newline_translation"
  | "translation_newline_original"
  | "slash_separated"
  | "custom";
export type ChatboxOverflowPolicy = "block" | "smart_truncate";

export interface ChatboxComposeInput {
  original: string;
  translation: string | null;
  source_language: string | null;
  target_language: string | null;
  send_mode: ChatboxSendMode;
  message_format: ChatboxMessageFormat;
  custom_format: string | null;
  overflow_policy: ChatboxOverflowPolicy;
}

export interface ChatboxPreview {
  text: string;
  char_count: number;
  limit: number;
  over_limit: boolean;
  truncated: boolean;
  sendable: boolean;
}

export interface ChatboxMessage {
  id: number;
  source: "manual" | "microphone" | "resend";
  original: string;
  translation: string | null;
  source_language: string | null;
  target_language: string | null;
  send_mode: ChatboxSendMode;
  message_format: ChatboxMessageFormat;
  custom_format: string | null;
  rendered_text: string;
  char_count: number;
  truncated: boolean;
  status: "sent" | "failed";
  error_code: string | null;
  error_detail: string | null;
  resent_from_id: number | null;
  created_at: string;
  sent_at: string | null;
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
export type ApiProvider = AsrApiProvider | "gemini" | "openai_compatible" | "deepl" | "microsoft_translator";
export type ApiProfilePurpose = "asr" | "llm" | "shared";
export type ProviderSupportLevel = "native" | "protocol_compatible";
export type ApiAuthMode = "bearer" | "none";

export interface HttpHeaderConfig {
  name: string;
  value: string;
}

export interface ProviderPreset {
  id: string;
  display_name: string;
  base_url: string;
  auth_mode: ApiAuthMode;
  is_local: boolean;
}

export interface ProviderCapabilities {
  supports_streaming: boolean;
  supports_model_listing: boolean;
  requires_api_key: boolean;
  is_local: boolean;
  supports_context: boolean;
  supports_translation: boolean;
  supports_asr: boolean;
  supported_languages: string[];
}

export interface ProviderSupportLevels {
  asr: ProviderSupportLevel | null;
  translation: ProviderSupportLevel | null;
}

export interface ProviderDefinition {
  id: ApiProvider;
  display_name: string;
  purposes: ApiProfilePurpose[];
  support_levels: ProviderSupportLevels;
  capabilities: ProviderCapabilities;
  presets: ProviderPreset[];
}

export interface ApiProfile {
  id: string;
  name: string;
  provider: ApiProvider;
  region?: string;
  workspace_id?: string;
  base_url?: string;
  purpose?: ApiProfilePurpose;
  preset_id?: string;
  auth_mode?: ApiAuthMode;
  is_local?: boolean;
  timeout_ms?: number;
  headers?: HttpHeaderConfig[];
}

export interface ApiProfileView extends ApiProfile {
  provider_display_name: string;
  active: boolean;
  translation_active: boolean;
  credential: CredentialStatus;
  capabilities: ProviderCapabilities;
  support_levels: ProviderSupportLevels;
}

export interface ApiModelCatalog {
  models: string[];
}

export interface ConnectionDiagnosticCheck {
  name: "configuration" | "endpoint" | "authentication" | "models" | "completion" | "streaming";
  status: "passed" | "warning" | "failed" | "skipped";
  code?: string;
  detail?: string;
}

export interface ConnectionDiagnostic {
  ok: boolean;
  latency_ms?: number;
  checks?: ConnectionDiagnosticCheck[];
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

export interface ExternalApiRuntimeStatus {
  state: "disabled" | "running" | "failed";
  address: string | null;
  error: string | null;
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
  schema_version: 16;
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
    mute_sync_enabled: boolean;
    mute_status_toast_enabled: boolean;
  };
  dictionary: {
    selection_lookup_enabled: boolean;
  };
  anki: AnkiSettings;
  external_api: ExternalApiSettings;
}

export interface ExternalApiSettings {
  enabled: boolean;
  host: string;
  port: number;
  require_token: boolean;
}

export interface TranslationSettings {
  mode: "disabled" | "manual" | "automatic";
  target_language: "zh-Hans" | "zh-Hant" | "en" | "ja" | "ko" | "es" | "fr" | "de" | "ru";
  profile_id: string | null;
  model: string;
  thinking_enabled: boolean;
  translate_microphone: boolean;
  microphone_target_language: "zh-Hans" | "zh-Hant" | "en" | "ja" | "ko" | "es" | "fr" | "de" | "ru";
  prompt: TranslationPromptSettings;
}

export type GlossaryCategory = "person" | "world" | "game" | "custom";

export interface GlossaryEntry {
  source: string;
  target: string | null;
  category: GlossaryCategory;
  case_sensitive: boolean;
}

export interface TranslationPromptSettings {
  system_prompt: string;
  context_enabled: boolean;
  include_speaker: boolean;
  include_microphone: boolean;
  include_chatbox: boolean;
  max_messages: number;
  max_chars: number;
  glossary: GlossaryEntry[];
}

export interface TranslationPromptPreview {
  instructions: string;
  context_message_count: number;
  context_char_count: number;
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
  capture_requested: boolean;
  microphone_capture_state: "running" | "stopped" | "paused_vrchat_muted";
  audio_device: AudioDevice | null;
  microphone_device?: AudioDevice | null;
  microphone_test_running?: boolean;
  microphone_test_device?: AudioDevice | null;
  asr_status: string;
  vad_backend: string;
  vad_model_version: string | null;
  last_error: string | null;
  osc?: OscRuntimeStatus;
  vrchat_mute_sync: VrchatMuteStatus;
}

export interface VrchatMuteStatus {
  enabled: boolean;
  connection: "disabled" | "discovering" | "connected" | "unavailable";
  muted: boolean | null;
  last_error: string | null;
}

export interface OscRuntimeStatus {
  enabled: boolean;
  target: string;
  status: "disabled" | "ready" | "error";
  last_error: string | null;
  last_sent_at: string | null;
  dropped_messages: number;
  send_gate: "open" | "blocked_vrchat_muted" | "blocked_mute_unknown";
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
