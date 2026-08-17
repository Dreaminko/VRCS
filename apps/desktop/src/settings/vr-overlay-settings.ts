import type {
  Settings,
  VrOverlayHeadsetSettings,
  VrOverlaySettings,
  VrOverlayWristSettings,
} from "../types";

export const DEFAULT_VR_OVERLAY_HEADSET_SETTINGS: VrOverlayHeadsetSettings = {
  enabled: true,
  content_mode: "bilingual",
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
  background_opacity: 0.55,
  vr_drag_edit_enabled: false,
};

export const DEFAULT_VR_OVERLAY_WRIST_SETTINGS: VrOverlayWristSettings = {
  enabled: true,
  hand: "left",
  dominant_hand: "right",
  content_mode: "bilingual",
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

export const DEFAULT_VR_OVERLAY_SETTINGS: VrOverlaySettings = {
  enabled: false,
  headset: { ...DEFAULT_VR_OVERLAY_HEADSET_SETTINGS },
  wrist: { ...DEFAULT_VR_OVERLAY_WRIST_SETTINGS },
};

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
