import type { CredentialStatus } from "../shared/protocol/credentials";

export interface RecognitionServiceSettings {
  model: string;
  context: string;
}

export interface AsrSettings {
  backend: string;
  language: "auto" | "en" | "ja" | "zh" | "ko" | "es" | "fr" | "de";
  managed_qwen: {
    package_id: string;
    device: "auto" | "cpu" | "gpu";
  };
  active_profile_id: string | null;
  service_settings: Record<string, RecognitionServiceSettings>;
}

export type ApiProvider = string;
export type ApiCapability = "speech_to_text" | "text_generation" | "text_translation" | "image_understanding";
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
  supports_image_understanding: boolean;
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
  recognition_languages?: string[];
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
  model_details?: Record<string, ApiModelDetails>;
}

export interface ApiModelDetails {
  input_modalities?: string[] | null;
  output_modalities?: string[] | null;
  context_tokens?: number | null;
  max_output_tokens?: number | null;
  temperature?: boolean | null;
  structured_output?: boolean | null;
  tool_call?: boolean | null;
  status?: string | null;
  canonical_model_id?: string | null;
  field_sources?: Record<string, string>;
  updated_at?: number | null;
  stale?: boolean;
  reasoning?: {
    can_disable?: boolean | null;
    supports_max_tokens?: boolean | null;
    supports_dynamic_budget?: boolean | null;
    mandatory?: boolean | null;
    supported_efforts?: string[] | null;
    default_effort?: string | null;
    supported?: boolean | null;
    controls?: Array<{ type: "toggle" } | { type: "effort"; values: string[] } | { type: "budget_tokens"; min?: number | null; max?: number | null }>;
  } | null;
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

export interface QwenModelRecord {
  id: string;
  engine: string;
  repository: string;
  revision: string;
  status: "not_downloaded" | "downloading" | "verifying" | "installed" | "corrupt" | "error";
  downloaded_bytes: number;
  total_bytes: number;
  progress: number;
  error: string | null;
}

export interface QwenRuntimeStatus {
  available: boolean;
  running: boolean;
  status: "not_loaded" | "loading" | "ready" | "error";
  error: string | null;
  device: "cpu" | "gpu" | null;
  fallback: string | null;
  gpu_devices: string[];
  installation: {
    status: "not_downloaded" | "downloading" | "verifying" | "installed" | "error";
    downloaded_bytes: number;
    total_bytes: number;
    progress: number;
    error: string | null;
  };
}
