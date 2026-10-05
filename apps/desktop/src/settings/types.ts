import type { AnkiSettings } from "../anki/types";
import type { AudioOutputSettings, MicrophoneSettings, VadSettings } from "../capture/types";
import type {
  ExternalApiSettings,
  VrOverlaySettings,
  VrcxSettings,
} from "../integrations/types";
import type { AsrSettings } from "../providers/types";

export interface Settings {
  schema_version: 26;
  server: {
    host: string;
    port: number;
  };
  storage: {
    database_path: string;
    model_directory: string;
    subtitle_history_max_bytes: number;
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
    mute_status_toast_enabled: boolean;
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
  thinking_enabled: boolean;
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
