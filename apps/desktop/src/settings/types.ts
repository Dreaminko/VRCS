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

export interface Settings {
  schema_version: 36;
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

export interface ConversationTitleSettings {
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

export interface VisionAsrSettings {
  backend: string;
  profile_id: string | null;
  language: AsrSettings["language"];
  model: string;
}

export interface VisionSettings {
  enabled: boolean;
  vr_enabled: boolean;
  desktop_enabled: boolean;
  desktop_shortcut: string;
  asr: VisionAsrSettings | null;
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
  mode: "disabled" | "manual" | "automatic";
  speaker_targets: TranslationTargetSettings[];
  microphone_targets: TranslationTargetSettings[];
  prompt: TranslationPromptSettings;
}

export interface TranslationTargetSettings {
  target_language: string;
  profile_id: string | null;
  model: string;
  model_by_profile?: Record<string, string>;
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
  instructions: string;
  context_message_count: number;
  context_char_count: number;
}
