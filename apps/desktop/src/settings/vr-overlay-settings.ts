import type {
  VrOcrModelStatus,
  VrOcrSettings,
  VrOverlayHeadsetSettings,
  VrOverlaySettings,
  VrOverlayWristSettings,
} from "../integrations/types";
import type { Settings } from "./types";

export type VrOverlayDisplayKind = "headset" | "wrist";
export type VrOverlayPositionField = "offset_x_m" | "offset_y_m" | "offset_z_m" | "distance_m" | "pitch_deg" | "yaw_deg" | "roll_deg";

export const VR_OVERLAY_POSITION_RANGES = {
  headset: {
    offset_x_m: { min: -2, max: 2, step: 0.01 },
    offset_y_m: { min: -2, max: 2, step: 0.01 },
    distance_m: { min: 0.25, max: 5, step: 0.05 },
    pitch_deg: { min: -90, max: 90, step: 1 },
    yaw_deg: { min: -180, max: 180, step: 1 },
    roll_deg: { min: -180, max: 180, step: 1 },
  },
  wrist: {
    offset_x_m: { min: -0.5, max: 0.5, step: 0.01 },
    offset_y_m: { min: -0.5, max: 0.5, step: 0.01 },
    offset_z_m: { min: -0.5, max: 0.5, step: 0.01 },
    pitch_deg: { min: -180, max: 180, step: 1 },
    yaw_deg: { min: -180, max: 180, step: 1 },
    roll_deg: { min: -180, max: 180, step: 1 },
  },
} as const;

export function adjustVrOverlayPosition(settings: Settings, kind: VrOverlayDisplayKind, field: VrOverlayPositionField, direction: number): Settings {
  if ((kind !== "headset" && kind !== "wrist") || ![-1, 1].includes(direction)) return settings;
  const ranges: Partial<Record<VrOverlayPositionField, { min: number; max: number; step: number }>> = VR_OVERLAY_POSITION_RANGES[kind];
  const range = ranges[field];
  if (!range) return settings;
  const current = (settings.vr_overlay[kind] as unknown as Record<string, unknown>)[field];
  if (typeof current !== "number" || !Number.isFinite(current)) return settings;
  const value = Math.round(Math.min(range.max, Math.max(range.min, current + direction * range.step)) * 100) / 100;
  if (current === value) return settings;
  return kind === "headset" ? patchVrOverlayHeadset(settings, { [field]: value }) : patchVrOverlayWrist(settings, { [field]: value });
}

export function resetVrOverlayPosition(settings: Settings, kind: VrOverlayDisplayKind): Settings {
  if (kind !== "headset" && kind !== "wrist") return settings;
  const defaults = kind === "headset" ? DEFAULT_VR_OVERLAY_HEADSET_SETTINGS : DEFAULT_VR_OVERLAY_WRIST_SETTINGS;
  const patch = Object.fromEntries(Object.keys(VR_OVERLAY_POSITION_RANGES[kind]).map((field) => [field, (defaults as unknown as Record<string, unknown>)[field]]));
  const current = settings.vr_overlay[kind] as unknown as Record<string, unknown>;
  if (Object.entries(patch).every(([field, value]) => current[field] === value)) return settings;
  return kind === "headset" ? patchVrOverlayHeadset(settings, patch) : patchVrOverlayWrist(settings, patch);
}

export const DEFAULT_VR_OVERLAY_HEADSET_SETTINGS: VrOverlayHeadsetSettings = {
  enabled: true,
  show_partials: false,
  show_translation_partials: false,
  include_speaker: true,
  include_microphone: false,
  include_chatbox: false,
  offset_x_m: 0,
  offset_y_m: -0.28,
  distance_m: 1.2,
  pitch_deg: -8,
  yaw_deg: 0,
  roll_deg: 0,
  width_m: 1.2,
  opacity: 0.92,
  display_seconds: 6,
  fade_seconds: 1,
  font_size_px: 54,
  lines_per_language: 2,
  background_opacity: 0.55,
  vr_drag_edit_enabled: false,
};

export const DEFAULT_VR_OVERLAY_WRIST_SETTINGS: VrOverlayWristSettings = {
  enabled: true,
  hand: "left",
  dominant_hand: "right",
  show_partials: false,
  show_translation_partials: false,
  include_speaker: true,
  include_microphone: false,
  include_chatbox: false,
  max_entries: 5,
  idle_hide_seconds: 0,
  offset_x_m: 0.03,
  offset_y_m: 0.08,
  offset_z_m: -0.06,
  pitch_deg: -55,
  yaw_deg: 0,
  roll_deg: 0,
  width_m: 0.32,
  opacity: 0.94,
  font_size_px: 32,
  background_opacity: 0.65,
};

export const DEFAULT_OCR_SETTINGS: VrOcrSettings = {
  enabled: false,
  desktop_enabled: false,
  shortcut: "Ctrl+Alt+O",
  backend: "cloud",
  display_mode: "stereo",
  timeout_seconds: 30,
  minimum_confidence: 0.6,
  region_fraction: 0.6,
  targets: [{ target_language: "zh-Hans", profile_id: null, model: "gpt-5-mini", thinking_enabled: false }],
  hand_gesture_enabled: true,
  display_seconds: 15,
  background_opacity: 1,
};

export const DEFAULT_VR_OVERLAY_SETTINGS: VrOverlaySettings = {
  enabled: false,
  translation_display: "all_languages",
  headset: { ...DEFAULT_VR_OVERLAY_HEADSET_SETTINGS },
  wrist: { ...DEFAULT_VR_OVERLAY_WRIST_SETTINGS },
};

export function isVrOcrBackendReady(
  backend: VrOcrSettings["backend"],
  credentialConfigured: boolean,
  modelState?: VrOcrModelStatus["state"],
): boolean {
  return backend === "local" ? modelState === "ready" : credentialConfigured;
}

export function patchVrOverlay(
  settings: Settings,
  patch: Partial<VrOverlaySettings>,
): Settings {
  return {
    ...settings,
    vr_overlay: { ...settings.vr_overlay, ...patch },
  };
}

export function patchVrOverlayHeadset(
  settings: Settings,
  patch: Partial<VrOverlayHeadsetSettings>,
): Settings {
  return patchVrOverlay(settings, {
    headset: { ...settings.vr_overlay.headset, ...patch },
  });
}

export function patchVrOverlayWrist(
  settings: Settings,
  patch: Partial<VrOverlayWristSettings>,
): Settings {
  return patchVrOverlay(settings, {
    wrist: { ...settings.vr_overlay.wrist, ...patch },
  });
}

export function setVrOverlayHeadsetDisplaySeconds(
  settings: Settings,
  displaySeconds: number,
): Settings {
  return patchVrOverlayHeadset(settings, {
    display_seconds: displaySeconds,
    fade_seconds: Math.min(settings.vr_overlay.headset.fade_seconds, displaySeconds),
  });
}

export function resetVrOverlayHeadset(settings: Settings): Settings {
  return patchVrOverlay(settings, {
    headset: { ...DEFAULT_VR_OVERLAY_HEADSET_SETTINGS },
  });
}

export function resetVrOverlayWrist(settings: Settings): Settings {
  return patchVrOverlay(settings, {
    wrist: { ...DEFAULT_VR_OVERLAY_WRIST_SETTINGS },
  });
}

export function patchOcr(settings: Settings, patch: Partial<VrOcrSettings>): Settings {
  return { ...settings, ocr: { ...settings.ocr, ...patch } };
}
