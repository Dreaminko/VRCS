import type {
  VrOverlayTranslationDisplay,
  VrOverlayHand,
} from "../integrations/types";
import type { Settings } from "../settings/types";
import type { ApiProfileView } from "../providers/types";
import { supportsTranslation } from "../api-profile-purpose.ts";
import { liveTranslationServiceName } from "../recognition-services.ts";
import { TRANSLATION_LANGUAGE_CODES } from "../translation-languages.ts";
import { applyLanguagePreset, saveLanguagePreset, targetLanguageCodes } from "../settings/translation/language-settings.ts";
import {
  patchVrOverlay,
  patchOcr,
  patchVrOverlayHeadset,
  patchVrOverlayWrist,
  adjustVrOverlayPosition,
  resetVrOverlayPosition,
  type VrOverlayDisplayKind,
  type VrOverlayPositionField,
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
  | "open_ocr_bindings"
  | "toggle_osc"
  | "toggle_osc_original"
  | "toggle_osc_mute_sync"
  | "toggle_osc_mute_toast"
  | "cycle_osc_strategy"
  | { set_recognition_language: string }
  | { set_translation_mode: string }
  | { set_target_language: { group: "speaker" | "microphone"; index: number; value: string } }
  | { add_target: "speaker" | "microphone" }
  | { delete_target: { group: "speaker" | "microphone"; index: number } }
  | { move_target: { group: "speaker" | "microphone"; index: number; offset: number } }
  | { save_language_preset: string }
  | { apply_language_preset: string }
  | { delete_language_preset: string }
  | { adjust_display_position: { kind: VrOverlayDisplayKind; field: VrOverlayPositionField; direction: number } }
  | { reset_display_position: VrOverlayDisplayKind };

export const RECOGNITION_LANGUAGES: Settings["asr"]["language"][] = ["auto", "en", "ja", "zh", "ko", "es", "fr", "de"];
const TRANSLATION_MODES: Settings["translation"]["mode"][] = ["disabled", "manual", "automatic"];
const OSC_STRATEGIES: Settings["osc"]["translation_strategy"][] = ["preferred_only", "round_robin", "all_languages"];

export function dashboardLanguageChoices(settings: Settings, profiles: ApiProfileView[]) {
  const liveService = liveTranslationServiceName(settings.asr.backend);
  const nativeTranslation = Boolean(liveService && settings.translation.mode === "automatic");
  const targetChoices = (key: "speaker_targets" | "microphone_targets") => settings.translation[key].map((target, index) =>
    targetLanguageCodes(target, settings.translation[key].map((item) => item.target_language), profiles.filter(supportsTranslation), index === 0 && nativeTranslation ? settings.asr.backend : undefined));
  const speaker_targets = targetChoices("speaker_targets");
  const microphone_targets = targetChoices("microphone_targets");
  return {
    recognition: liveService ? [] : RECOGNITION_LANGUAGES,
    mode: liveService || profiles.some(supportsTranslation) ? TRANSLATION_MODES : [],
    speaker_targets, microphone_targets,
  };
}

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

export function applyVrDashboardAction(settings: Settings, action: VrDashboardAction, profiles: ApiProfileView[] = []): Settings {
  if (typeof action === "object") {
    if ("adjust_display_position" in action) {
      const { kind, field, direction } = action.adjust_display_position;
      return adjustVrOverlayPosition(settings, kind, field, direction);
    }
    if ("reset_display_position" in action) return resetVrOverlayPosition(settings, action.reset_display_position);
    if ("save_language_preset" in action) return saveLanguagePreset(settings, action.save_language_preset);
    if ("apply_language_preset" in action) return applyLanguagePreset(settings, action.apply_language_preset);
    if ("delete_language_preset" in action) {
      if (!settings.language_presets.some((item) => item.id === action.delete_language_preset)) return settings;
      return { ...settings, language_presets: settings.language_presets.filter((item) => item.id !== action.delete_language_preset) };
    }
    if ("add_target" in action || "delete_target" in action || "move_target" in action) {
      const group = "add_target" in action ? action.add_target : "delete_target" in action ? action.delete_target.group : action.move_target.group;
      if (group !== "speaker" && group !== "microphone") return settings;
      const key = `${group}_targets` as const;
      const targets = settings.translation[key];
      let next = [...targets];
      if ("add_target" in action) {
        const profile = profiles.find(supportsTranslation);
        if (!profile || targets.length >= 3) return settings;
        const languages = profile.capabilities.supported_languages.length ? profile.capabilities.supported_languages : TRANSLATION_LANGUAGE_CODES;
        const target_language = languages.find((code) => !targets.some((item) => item.target_language === code)) ?? "en";
        next.push({ target_language, profile_id: profile.id, model: "gpt-5-mini", thinking_enabled: false });
      } else {
        const index = "delete_target" in action ? action.delete_target.index : action.move_target.index;
        if (!Number.isInteger(index) || !targets[index]) return settings;
        if ("delete_target" in action) {
          if (targets.length <= 1) return settings;
          next = targets.filter((_, itemIndex) => itemIndex !== index);
        } else {
          const offset = action.move_target.offset;
          if (![-1, 1].includes(offset) || !targets[index + offset]) return settings;
          [next[index], next[index + offset]] = [next[index + offset], next[index]];
        }
      }
      return { ...settings, translation: { ...settings.translation, [key]: next } };
    }
    const choices = dashboardLanguageChoices(settings, profiles);
    if ("set_recognition_language" in action) {
      const language = action.set_recognition_language as Settings["asr"]["language"];
      if (!choices.recognition.includes(language) || settings.asr.language === language) return settings;
      return { ...settings, asr: { ...settings.asr, language } };
    }
    if ("set_translation_mode" in action) {
      const mode = action.set_translation_mode as Settings["translation"]["mode"];
      if (!choices.mode.includes(mode) || settings.translation.mode === mode) return settings;
      return { ...settings, translation: { ...settings.translation, mode } };
    }
    const { group, index, value: language } = action.set_target_language;
    if (group !== "speaker" && group !== "microphone") return settings;
    const key = `${group}_targets` as const;
    const values = choices[key][index] ?? [];
    const targets = settings.translation[key];
    if (!Number.isInteger(index) || !values.includes(language) || targets[index]?.target_language === language) return settings;
    return {
      ...settings,
      translation: { ...settings.translation, [key]: targets.map((target, itemIndex) => itemIndex === index ? { ...target, target_language: language } : target) },
    };
  }
  switch (action) {
    case "toggle_osc":
    case "toggle_osc_original":
    case "toggle_osc_mute_sync":
    case "toggle_osc_mute_toast": {
      const key = {
        toggle_osc: "enabled", toggle_osc_original: "preserve_original_text",
        toggle_osc_mute_sync: "mute_sync_enabled", toggle_osc_mute_toast: "mute_status_toast_enabled",
      }[action] as "enabled" | "preserve_original_text" | "mute_sync_enabled" | "mute_status_toast_enabled";
      return { ...settings, osc: { ...settings.osc, [key]: !settings.osc[key] } };
    }
    case "cycle_osc_strategy":
      return { ...settings, osc: { ...settings.osc, translation_strategy: cycle(OSC_STRATEGIES, settings.osc.translation_strategy) } };
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
          action.endsWith("down") ? -0.05 : 0.05, 0.25, 3),
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
          action.endsWith("down") ? -0.01 : 0.01, 0.1, 1),
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
