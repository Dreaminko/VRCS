import type { AudioDevice } from "../capture/types";
import type { OscRuntimeStatus, VrchatMuteStatus } from "../integrations/types";
import type {
  LanguagePreset,
  OscTranslationStrategy,
  TranslationSettings,
} from "../settings/types";

export type ConnectionState = "connecting" | "connected" | "disconnected";

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
  language_session?: ActiveLanguageSession;
}

export type ActiveLanguageSession =
  | { kind: "global" }
  | ({ kind: "preset"; preset_id: string; preset_name: string } & LanguageSessionSnapshot)
  | ({ kind: "override" } & LanguageSessionSnapshot);

export interface LanguageSessionSnapshot {
  recognition_language: LanguagePreset["recognition_language"] | string;
  translation: TranslationSettings;
  osc_translation_strategy: OscTranslationStrategy;
}
