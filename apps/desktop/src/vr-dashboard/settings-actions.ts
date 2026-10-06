import type {
  VrOverlayTranslationDisplay,
  VrOverlayHand,
} from "../integrations/types";
import type { Settings } from "../settings/types";
import {
  patchVrOverlay,
  patchOcr,
  patchVrOverlayHeadset,
  patchVrOverlayWrist,
} from "../settings/vr-overlay-settings.ts";

export type VrDashboardAction =
  | "toggle_master"
  | "toggle_headset"
  | "cycle_headset_content"
  | "headset_width_down"
  | "headset_width_up"
  | "headset_opacity_down"
  | "headset_opacity_up"
  | "preview_headset"
  | "toggle_wrist"
  | "cycle_wrist_hand"
  | "cycle_wrist_content"
  | "wrist_width_down"
  | "wrist_width_up"
  | "wrist_opacity_down"
  | "wrist_opacity_up"
  | "preview_wrist"
  | "toggle_ocr"
  | "toggle_ocr_gesture"
  | "open_ocr_bindings";

const TRANSLATION_DISPLAYS: VrOverlayTranslationDisplay[] = ["preferred_only", "all_languages"];
const HANDS: VrOverlayHand[] = ["left", "right", "dominant"];

function cycle<T>(values: readonly T[], current: T): T {
  const index = values.indexOf(current);
  return values[(index + 1) % values.length];
}

function step(value: number, delta: number, minimum: number, maximum: number): number {
  const stepped = Math.min(maximum, Math.max(minimum, value + delta));
  return Math.round(stepped * 100) / 100;
}

export function applyVrDashboardAction(settings: Settings, action: VrDashboardAction): Settings {
  switch (action) {
    case "toggle_master":
      return patchVrOverlay(settings, { enabled: !settings.vr_overlay.enabled });
    case "toggle_headset":
      return patchVrOverlayHeadset(settings, { enabled: !settings.vr_overlay.headset.enabled });
    case "cycle_headset_content":
      return patchVrOverlay(settings, {
        translation_display: cycle(TRANSLATION_DISPLAYS, settings.vr_overlay.translation_display),
      });
    case "headset_width_down":
    case "headset_width_up":
      return patchVrOverlayHeadset(settings, {
        width_m: step(settings.vr_overlay.headset.width_m,
          action.endsWith("down") ? -0.1 : 0.1, 0.2, 3),
      });
    case "headset_opacity_down":
    case "headset_opacity_up":
      return patchVrOverlayHeadset(settings, {
        opacity: step(settings.vr_overlay.headset.opacity,
          action.endsWith("down") ? -0.05 : 0.05, 0.1, 1),
      });
    case "toggle_wrist":
      return patchVrOverlayWrist(settings, { enabled: !settings.vr_overlay.wrist.enabled });
    case "cycle_wrist_hand":
      return patchVrOverlayWrist(settings, {
        hand: cycle(HANDS, settings.vr_overlay.wrist.hand),
      });
    case "cycle_wrist_content":
      return patchVrOverlay(settings, {
        translation_display: cycle(TRANSLATION_DISPLAYS, settings.vr_overlay.translation_display),
      });
    case "wrist_width_down":
    case "wrist_width_up":
      return patchVrOverlayWrist(settings, {
        width_m: step(settings.vr_overlay.wrist.width_m,
          action.endsWith("down") ? -0.02 : 0.02, 0.1, 1),
      });
    case "wrist_opacity_down":
    case "wrist_opacity_up":
      return patchVrOverlayWrist(settings, {
        opacity: step(settings.vr_overlay.wrist.opacity,
          action.endsWith("down") ? -0.05 : 0.05, 0.1, 1),
      });
    case "toggle_ocr":
      return patchOcr(settings, { enabled: !settings.ocr.enabled });
    case "toggle_ocr_gesture":
      return patchOcr(settings, { hand_gesture_enabled: !settings.ocr.hand_gesture_enabled });
    case "preview_headset":
    case "preview_wrist":
    case "open_ocr_bindings":
      return settings;
  }
}
