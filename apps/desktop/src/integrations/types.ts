export interface ExternalApiSettings {
  enabled: boolean;
  host: string;
  port: number;
  require_token: boolean;
}

export interface ExternalApiRuntimeStatus {
  state: "disabled" | "running" | "failed";
  address: string | null;
  error: string | null;
}

export interface VrcxSettings {
  enabled: boolean;
  port: number;
  include_in_llm_context: boolean;
  include_in_asr_context: boolean;
}

export interface VrcxRuntimeStatus {
  state: "disabled" | "missing_token" | "connecting" | "connected" | "error";
  app_version: string | null;
  protocol: number | null;
  world_name: string | null;
  member_count: number;
  last_updated_at: string | null;
  error: string | null;
}

export type VrOverlayTranslationDisplay = "preferred_only" | "all_languages";
export type VrOverlayHand = "left" | "right" | "dominant";
export type VrOverlayControllerHand = "left" | "right";

export interface VrOverlayHeadsetSettings {
  enabled: boolean;
  show_partials: boolean;
  show_translation_partials: boolean;
  include_speaker: boolean;
  include_microphone: boolean;
  include_chatbox: boolean;
  offset_x_m: number;
  offset_y_m: number;
  distance_m: number;
  pitch_deg: number;
  yaw_deg: number;
  roll_deg: number;
  width_m: number;
  opacity: number;
  display_seconds: number;
  fade_seconds: number;
  font_size_px: number;
  lines_per_language: number;
  background_opacity: number;
  vr_drag_edit_enabled: boolean;
}

export interface VrOverlayWristSettings {
  enabled: boolean;
  hand: VrOverlayHand;
  dominant_hand: VrOverlayControllerHand;
  show_partials: boolean;
  show_translation_partials: boolean;
  include_speaker: boolean;
  include_microphone: boolean;
  include_chatbox: boolean;
  max_entries: number;
  idle_hide_seconds: number;
  offset_x_m: number;
  offset_y_m: number;
  offset_z_m: number;
  pitch_deg: number;
  yaw_deg: number;
  roll_deg: number;
  width_m: number;
  opacity: number;
  font_size_px: number;
  background_opacity: number;
}

export interface VrOverlaySettings {
  enabled: boolean;
  translation_display: VrOverlayTranslationDisplay;
  headset: VrOverlayHeadsetSettings;
  wrist: VrOverlayWristSettings;
}

export interface VrOcrSettings {
  enabled: boolean;
  desktop_enabled: boolean;
  shortcut: string;
  backend: "cloud" | "local";
  device: "cpu" | "directml";
  display_mode: "wrist" | "stereo";
  timeout_seconds: number;
  minimum_confidence: number;
  region_fraction: number;
  targets: import("../settings/types").TranslationTargetSettings[];
  hand_gesture_enabled: boolean;
  display_seconds: number;
  background_opacity: number;
  wrist: VrOcrWristSettings | null;
}

export type VrOcrWristSettings = Pick<VrOverlayWristSettings,
  "hand" | "dominant_hand" | "offset_x_m" | "offset_y_m" | "offset_z_m" |
  "pitch_deg" | "yaw_deg" | "roll_deg" | "width_m" | "opacity" | "font_size_px">;

export interface VrOcrModelStatus {
  state: "missing" | "downloading" | "ready" | "error";
  downloaded_bytes: number;
  total_bytes: number;
  error: string | null;
}

export interface OcrExecutionStatus {
  requested_device: VrOcrSettings["device"];
  active_device: VrOcrSettings["device"] | null;
  fallback_reason: string | null;
}

export type VrOverlayRuntimeState =
  | "unsupported"
  | "disabled"
  | "waiting_runtime"
  | "initializing"
  | "ready"
  | "reconnecting"
  | "error"
  | "shutting_down";

export type VrOverlayResourceState =
  | "disabled"
  | "creating"
  | "ready_hidden"
  | "visible"
  | "fading"
  | "device_unavailable"
  | "recreating"
  | "error";

export interface VrOverlayResourceStatus {
  state: VrOverlayResourceState;
  sample_visible: boolean;
  last_error_code: string | null;
}

export interface VrOverlayWristStatus extends VrOverlayResourceStatus {
  bound_role: VrOverlayControllerHand | null;
  tracked_device_available: boolean;
}

export interface VrDashboardStatus {
  state: VrOverlayResourceState;
  visible: boolean;
  last_error_code: string | null;
}

export interface VrOverlayStatus {
  state: VrOverlayRuntimeState;
  runtime_installed: boolean;
  hmd_present: boolean;
  last_connected_at: string | null;
  reconnect_attempt: number;
  headset: VrOverlayResourceStatus;
  wrist: VrOverlayWristStatus;
  ocr: VrOcrStatus;
  dashboard: VrDashboardStatus;
  last_error_detail: string | null;
}

export interface VrOcrStatus {
  state: "disabled" | "waiting_vr" | "ready" | "unbound" | "capturing" | "submitting" | "pending" | "running"
    | "selecting" | "waiting_hands" | "downloading" | "loading_model" | "recognizing" | "translating" | "recognized" | "visible" | "invalid" | "error"
    | "no_text" | "low_confidence" | "source_visible" | "partial_visible" | "translation_failed" | "timed_out";
  scan_id: number;
  block_count: number;
  controller_bound: boolean;
  gesture_available: boolean;
  layout_limited: boolean;
  completed_translations: number;
  failed_translations: number;
  timed_out: boolean;
  wrist_state?: "hidden" | "visible" | "device_unavailable" | "error";
  wrist_error?: string | null;
  progress?: {
    scan_id: number;
    status: string;
    detail: string;
    elapsed_seconds: number;
    stages: ("pending" | "active" | "complete" | "skipped" | "failed")[];
      stage_labels: string[];
    translation_fraction: [number, number] | null;
  } | null;
  progress_error?: string | null;
  last_error_code: string | null;
  last_error: string | null;
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
