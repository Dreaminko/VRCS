import type { CredentialStatus } from "../shared/protocol/credentials";

export interface RecognitionServiceSettings {
  model: string;
  context: string;
}

export interface AsrSettings {
  backend: string;
  language: "auto" | "en" | "ja" | "zh" | "ko" | "es" | "fr" | "de";
  local: {
    model: "tiny" | "base" | "small" | "medium" | "large-v3";
    device: "auto" | "cpu" | "cuda";
    compute_type: "int8";
  };
  active_profile_id: string | null;
  service_settings: Record<string, RecognitionServiceSettings>;
  cloud_failure_policy: "reconnect" | "local";
}

export type ApiProvider = string;
export type ApiCapability = "speech_to_text" | "text_generation" | "text_translation";
export type ProviderCategory = "cloud_provider" | "local_service" | "custom_protocol";
export type RecognitionTransport = "realtime_stream" | "segmented_upload";
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
  supports_text_generation: boolean;
  supports_custom_translation_language: boolean;
  supported_languages: string[];
}

export interface ProviderSupportLevels {
  asr: ProviderSupportLevel | null;
  translation: ProviderSupportLevel | null;
}

export interface ProviderConnectionField {
  id: string;
  label?: string;
  label_key?: string;
  type?: "text" | "select" | "boolean" | "number";
  required?: boolean;
  default?: string | number | boolean | null;
  placeholder?: string;
  options?: Array<{ value: string; label: string }>;
  min?: number;
  max?: number;
  step?: number;
}

export interface ProviderConnectionDefinition {
  base_url: {
    mode: "fixed" | "editable";
    default?: string | null;
  };
  auth_modes: ApiAuthMode[];
  default_auth_mode: ApiAuthMode;
  fields: ProviderConnectionField[];
}

export interface ProviderServiceDefinition {
  id: string;
  display_name: string;
  capabilities: ApiCapability[];
  adapter: string;
  recognition_transport: RecognitionTransport | null;
  partial_results: boolean;
  models: string[];
  model_listing: boolean;
  supports_context: boolean;
}

export interface ProviderDefinition {
  id: ApiProvider;
  display_name: string;
  category: ProviderCategory;
  connection: ProviderConnectionDefinition;
  services: ProviderServiceDefinition[];
  support_levels: ProviderSupportLevels;
  capabilities: ProviderCapabilities;
  presets?: ProviderPreset[];
}

export interface ApiProfileInput {
  name: string;
  provider: ApiProvider;
  enabled_capabilities: ApiCapability[];
  region?: string;
  workspace_id?: string;
  base_url?: string;
  preset_id?: string;
  auth_mode?: ApiAuthMode;
  is_local?: boolean;
  timeout_ms?: number;
  headers?: HttpHeaderConfig[];
  [field: string]: unknown;
}

export interface ApiProfile extends ApiProfileInput {
  id: string;
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
