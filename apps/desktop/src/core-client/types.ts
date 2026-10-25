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
  player_audio?: PlayerAudioStatus | null;
  osc?: OscRuntimeStatus;
  vrchat_mute_sync: VrchatMuteStatus;
  language_session?: ActiveLanguageSession;
}

export interface PlayerAudioStatus {
  players?: { speaker: import("../subtitles/types").SpeakerIdentity; selected: boolean; active: boolean }[];
  phase: "waiting_game" | "waiting_world" | "waiting_traffic" | "waiting_key" | "acquiring_key" | "retrying_key" | "streaming" | "failed" | "stopped";
  captured_packets: number;
  decrypted_packets: number;
  pcm_frames: number;
  active_players: number;
  dropped_frames: number;
  decode_errors: number;
  key_scan?: {
    stage: "paging" | "scanning" | "verified" | "cached";
    scanned_bytes: number;
    total_bytes: number;
    candidates_tested: number;
    elapsed_ms: number;
    scan_elapsed_ms: number;
    workers: number;
  } | null;
  detail: string | null;
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
