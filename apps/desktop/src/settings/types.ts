import type { AnkiSettings } from "../anki/types";
import type { AudioOutputSettings, MicrophoneSettings, VadSettings } from "../capture/types";
import type {
  ExternalApiSettings,
  VrOverlaySettings,
  VrOcrSettings,
  VrcxSettings,
} from "../integrations/types";
import type { AsrSettings } from "../providers/types";

export type ModelDownloadSource = "auto" | "official" | "modelscope";
export type TranslationBackend = "api" | "plamo_local";

export interface Settings {
  schema_version: 40;
  model_defaults: ModelDefaults;
  learning_ai: LearningAiSettings;
  vrchat_chatbox: VrchatChatboxConfig;
  conversation_titles: ConversationTitleSettings;
  features: FeatureSettings;
  server: {
    host: string;
    port: number;
  };
  storage: {
    database_path: string;
    model_directory: string;
    subtitle_history_max_bytes: number;
    model_download_source: ModelDownloadSource;
  };
  audio: {
    sample_rate: number;
    output: AudioOutputSettings;
    microphone: MicrophoneSettings;
  };
  vad: VadSettings;
  asr: AsrSettings;
  translation: TranslationSettings;
  local_translation: {
    device: "cpu";
  };
  language_presets: LanguagePreset[];
  glossary: GlossarySettings;
  osc: {
    enabled: boolean;
    port: number;
    mute_sync_enabled: boolean;
    preserve_original_text: boolean;
    translation_strategy: OscTranslationStrategy;
  };
  dictionary: {
    selection_lookup_enabled: boolean;
  };
  anki: AnkiSettings;
  external_api: ExternalApiSettings;
  vrcx: VrcxSettings;
  vr_overlay: VrOverlaySettings;
  ocr: VrOcrSettings;
  vision: VisionSettings;
}

export interface VrchatChatboxConfig {
  enabled: boolean;
}

export interface ConversationTitleSettings {
  use_default_model?: boolean;
  auto_generate: boolean;
  profile_id: string | null;
  model: string;
  output_language: string;
}

export interface FeatureSettings {
  glossary: boolean;
  learning: boolean;
  anki: boolean;
  microphone_speaker_filter: boolean;
  vrcx: boolean;
  ocr: boolean;
  visual_question_answering: boolean;
  vr_overlay: boolean;
  external_api: boolean;
}

export type FeatureKey = keyof FeatureSettings;

export interface VisionSettings {
  use_default_model?: boolean;
  enabled: boolean;
  vr_enabled: boolean;
  desktop_enabled: boolean;
  desktop_shortcut: string;
  profile_id: string | null;
  model: string;
  thinking_enabled: boolean;
  reasoning_control?: import("../translation/translation-thinking").ReasoningSetting | null;
  answer_language: string;
  system_prompt: string;
  max_capture_seconds: number;
  asr_finalize_timeout_ms: number;
  request_timeout_seconds: number;
  max_output_tokens: number;
  image_max_edge: number;
  image_max_bytes: number;
  display_seconds: number;
  wrist: import("../integrations/types").VrOcrWristSettings | null;
}

export interface TranslationSettings {
  model_selection?: ModelSelection;
  mode: "disabled" | "manual" | "automatic";
  speaker_targets: TranslationTargetSettings[];
  microphone_targets: TranslationTargetSettings[];
  prompt: TranslationPromptSettings;
}

export interface TranslationTargetSettings {
  use_default_model?: boolean;
  target_language: string;
  backend?: TranslationBackend;
  profile_id: string | null;
  model: string;
  model_by_profile?: Record<string, string>;
  thinking_enabled: boolean;
  reasoning_control?: import("../translation/translation-thinking").ReasoningSetting | null;
}

export interface ModelBinding {
  backend?: TranslationBackend;
  profile_id: string | null;
  model: string;
}

export interface ModelSelection extends ModelBinding {
  use_default_model?: boolean;
}

export interface ModelDefaults {
  text: ModelBinding;
  translation: ModelBinding;
  vision: ModelBinding;
}

export interface LearningAiSettings extends ModelSelection {
  thinking_enabled: boolean;
  reasoning_control?: import("../translation/translation-thinking").ReasoningSetting | null;
}

export type OscTranslationStrategy = "preferred_only" | "round_robin" | "all_languages";

export interface LanguagePreset {
  id: string;
  name: string;
  recognition_language: AsrSettings["language"];
  translation_mode: TranslationSettings["mode"];
  speaker_targets: TranslationTargetSettings[];
  microphone_targets: TranslationTargetSettings[];
  osc_translation_strategy: OscTranslationStrategy;
}

export type GlossaryCategory = "person" | "world" | "game" | "custom";

export interface GlossaryEntry {
  source: string;
  target: string | null;
  category: GlossaryCategory;
  case_sensitive: boolean;
}

export interface GlossaryLocalSource {
  id: string;
  type: "local";
  name: string;
  enabled: boolean;
  entries: GlossaryEntry[];
}

export interface GlossarySubscriptionSource {
  id: string;
  type: "subscription";
  url: string;
  display_name: string | null;
  enabled: boolean;
}

export type GlossarySource = GlossaryLocalSource | GlossarySubscriptionSource;

export interface GlossarySettings {
  llm_enabled: boolean;
  asr_enabled: boolean;
  sources: GlossarySource[];
}

export interface TranslationPromptSettings {
  system_prompt: string;
  context_enabled: boolean;
  include_speaker: boolean;
  include_microphone: boolean;
  include_chatbox: boolean;
  max_messages: number;
  max_chars: number;
}

export interface GlossarySourceStatus {
  id: string;
  type: GlossarySource["type"];
  name: string;
  enabled: boolean;
  url: string | null;
  state: string;
  entry_count: number;
  effective_entry_count: number;
  omitted_entry_count: number;
  last_attempt_at: string | null;
  last_success_at: string | null;
  error_code: string | null;
  detail: string | null;
}

export interface TranslationPromptPreview {
  format?: "api" | "pfml3";
  instructions: string;
  prompt?: string;
  context_message_count: number;
  context_char_count: number;
}

export interface LocalTranslationModelStatus {
  id: "plamo-3-translate-0.8b-q4_k_m";
  revision: string;
  bytes: number;
  sha256: string;
  license: string;
  license_url: string;
  license_accepted: boolean;
  status: "not_downloaded" | "downloading" | "verifying" | "installed" | "corrupt" | "error";
  downloaded_bytes: number;
  total_bytes: number;
  progress: number;
  error: string | null;
}

export interface LocalTranslationRuntimeStatus {
  status: "not_loaded" | "loading" | "ready" | "error";
  device: "cpu";
  model: "plamo-3-translate-0.8b-q4_k_m";
  runtime_available: boolean;
  error: string | null;
}

export interface LocalTranslationStatus {
  model: LocalTranslationModelStatus;
  runtime: LocalTranslationRuntimeStatus;
  languages: string[];
  license: {
    name: string;
    url: string;
    attribution: string;
  };
}
