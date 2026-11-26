import type {
  VrOverlayTranslationDisplay,
  VrOverlayHand,
} from "../integrations/types";
import type { Settings } from "../settings/types";
import type { ApiProfileView } from "../providers/types";
import { supportsTranslation } from "../providers/api-profile-purpose.ts";
import { liveTranslationServiceName } from "../providers/recognition-services.ts";
import {
  PLAMO_LOCAL_MODEL_ID,
  PLAMO_TRANSLATION_LANGUAGE_CODES,
  TRANSLATION_LANGUAGE_CODES,
} from "../translation/translation-languages.ts";
import {
  DEFAULT_MODEL_SELECTION,
  EMPTY_MODEL_BINDING,
  resolveModelSelection,
  translationModelSelection,
} from "../settings/model-selection.ts";
import { applyLanguagePreset, saveLanguagePreset, targetLanguageCodes } from "../settings/translation/language-settings.ts";
import {
  patchVrOverlay,
  patchOcr,
  patchVrOverlayHeadset,
  patchVrOverlayWrist,
  adjustVrOverlayPosition,
  adjustVrOverlayAppearance,
  resetVrOverlayPosition,
  type VrOverlayAppearanceField,
  type VrOverlayDisplayKind,
  type VrOverlayPositionField,
} from "../settings/vr-overlay-settings.ts";

import { patchVision, supportsVision } from "../settings/vision-settings.ts";

export type VrDashboardAction =
  | "toggle_master"
  | "toggle_capture"
  | "toggle_network_capture"
  | "toggle_headset"
  | "cycle_headset_content"
  | "headset_width_down"
  | "headset_width_up"
  | "headset_opacity_down"
  | "headset_opacity_up"
  | "preview_headset"
  | "toggle_wrist"
  | "toggle_wrist_follow_view"
  | "cycle_wrist_hand"
  | "cycle_wrist_content"
  | "wrist_width_down"
  | "wrist_width_up"
  | "wrist_opacity_down"
  | "wrist_opacity_up"
  | "preview_wrist"
  | "toggle_vision"
  | "toggle_ocr"
  | "toggle_ocr_gesture"
  | "open_ocr_bindings"
  | "toggle_osc"
  | "toggle_osc_original"
  | "toggle_osc_mute_sync"
  | "cycle_osc_strategy"
  | { set_display_content: string }
  | { set_wrist_hand: string }
  | { set_osc_strategy: string }
  | { set_recognition_language: string }
  | { set_translation_mode: string }
  | { set_target_language: { group: "speaker" | "microphone"; index: number; value: string } }
  | { add_target: "speaker" | "microphone" }
  | { delete_target: { group: "speaker" | "microphone"; index: number } }
  | { move_target: { group: "speaker" | "microphone"; index: number; offset: number } }
  | { save_language_preset: string }
  | { apply_language_preset: string }
  | { delete_language_preset: string }
  | { adjust_display_position: { kind: VrOverlayDisplayKind; field: VrOverlayPositionField; direction: number; coarse?: boolean } }
  | { adjust_display_appearance: { kind: VrOverlayDisplayKind; field: VrOverlayAppearanceField; direction: number; coarse?: boolean } }
  | { reset_display_position: VrOverlayDisplayKind };

export const RECOGNITION_LANGUAGES: Settings["asr"]["language"][] = ["auto", "en", "ja", "zh", "ko", "es", "fr", "de"];
const TRANSLATION_MODES: Settings["translation"]["mode"][] = ["disabled", "manual", "automatic"];
const OSC_STRATEGIES: Settings["osc"]["translation_strategy"][] = ["preferred_only", "round_robin", "all_languages"];

export function hasPlamoTranslationConfigured(settings: Settings): boolean {
  return settings.model_defaults?.translation?.backend === "plamo_local"
    || settings.translation.model_selection?.backend === "plamo_local"
    || [...settings.translation.speaker_targets, ...settings.translation.microphone_targets]
      .some((target) => target.backend === "plamo_local");
}

export function dashboardLanguageChoices(settings: Settings, profiles: ApiProfileView[]) {
  const liveService = liveTranslationServiceName(settings.asr.backend);
  const nativeTranslation = Boolean(liveService && settings.translation.mode === "automatic");
  const targetChoices = (key: "speaker_targets" | "microphone_targets") => settings.translation[key].map((target, index) =>
    targetLanguageCodes(target, settings.translation[key].map((item) => item.target_language), profiles.filter(supportsTranslation), index === 0 && nativeTranslation ? settings.asr.backend : undefined));
  const speaker_targets = targetChoices("speaker_targets");
  const microphone_targets = targetChoices("microphone_targets");
  return {
    recognition: liveService ? [] : RECOGNITION_LANGUAGES,
    mode: liveService || hasPlamoTranslationConfigured(settings) || profiles.some(supportsTranslation) ? TRANSLATION_MODES : [],
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
  if (!settings.features.vr_overlay) return settings;
  if (typeof action === "string") {
    if (action.includes("ocr") && (!settings.features.ocr || !settings.ocr.master_enabled)) return settings;
    if (action === "toggle_vision" && !settings.features.visual_question_answering) return settings;
  }
  if (typeof action === "object") {
    if ("set_display_content" in action) {
      const value = action.set_display_content as VrOverlayTranslationDisplay;
      if (!TRANSLATION_DISPLAYS.includes(value) || settings.vr_overlay.translation_display === value) return settings;
      return patchVrOverlay(settings, { translation_display: value });
    }
    if ("set_wrist_hand" in action) {
      const value = action.set_wrist_hand as VrOverlayHand;
      if (!HANDS.includes(value) || settings.vr_overlay.wrist.hand === value) return settings;
      return patchVrOverlayWrist(settings, { hand: value });
    }
    if ("set_osc_strategy" in action) {
      const value = action.set_osc_strategy as Settings["osc"]["translation_strategy"];
      if (!OSC_STRATEGIES.includes(value) || settings.osc.translation_strategy === value) return settings;
      return { ...settings, osc: { ...settings.osc, translation_strategy: value } };
    }
    if ("adjust_display_position" in action) {
      const { kind, field, direction, coarse } = action.adjust_display_position;
      return adjustVrOverlayPosition(settings, kind, field, direction, Boolean(coarse));
    }
    if ("adjust_display_appearance" in action) {
      const { kind, field, direction, coarse } = action.adjust_display_appearance;
      return adjustVrOverlayAppearance(settings, kind, field, direction, Boolean(coarse));
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
        if (targets.length >= 3) return settings;
        const modelSelection = settings.model_defaults?.translation
          ? translationModelSelection(settings)
          : (settings.translation.model_selection ?? EMPTY_MODEL_BINDING);
        const selectedProfile = modelSelection.profile_id
          ? profiles.find((item) => item.id === modelSelection.profile_id && supportsTranslation(item))
          : undefined;
        const fallbackProfile = selectedProfile ?? profiles.find(supportsTranslation);
        const binding = modelSelection.backend === "plamo_local"
          ? {
              backend: "plamo_local" as const,
              profile_id: null,
              model: modelSelection.model || PLAMO_LOCAL_MODEL_ID,
            }
          : selectedProfile
            ? {
                backend: "api" as const,
                profile_id: selectedProfile.id,
                model: modelSelection.model,
              }
            : fallbackProfile
              ? {
                  backend: "api" as const,
                  profile_id: fallbackProfile.id,
                  model: targets.find((item) => item.profile_id === fallbackProfile.id && item.model.trim())?.model ?? "",
                }
              : null;
        if (!binding) return settings;
        const profile = profiles.find((item) => item.id === binding.profile_id);
        const languages = binding.backend === "plamo_local"
          ? [...PLAMO_TRANSLATION_LANGUAGE_CODES]
          : profile?.capabilities.supported_languages.length
            ? profile.capabilities.supported_languages
            : TRANSLATION_LANGUAGE_CODES;
        const target_language = languages.find((code) => !targets.some((item) => item.target_language === code)) ?? "en";
        next.push({
          target_language,
          ...binding,
          use_default_model: (settings.translation.model_selection ?? DEFAULT_MODEL_SELECTION).use_default_model ?? true,
          thinking_enabled: false,
        });
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
    case "toggle_osc_mute_sync": {
      const key = {
        toggle_osc: "enabled", toggle_osc_original: "preserve_original_text",
        toggle_osc_mute_sync: "mute_sync_enabled",
      }[action] as "enabled" | "preserve_original_text" | "mute_sync_enabled";
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
    case "toggle_wrist_follow_view":
      if (settings.vr_overlay.wrist.anchor_mode !== "world") return settings;
      return patchVrOverlayWrist(settings, { follow_view: !settings.vr_overlay.wrist.follow_view });
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
    case "toggle_vision": {
      if (!settings.vision.enabled) return settings;
      const visionBinding = settings.model_defaults?.vision
        ? resolveModelSelection(settings.vision, settings.model_defaults.vision)
        : settings.vision;
      if (!settings.vision.vr_enabled && (!visionBinding.model.trim()
        || !profiles.some((profile) => profile.id === visionBinding.profile_id && supportsVision(profile)))) return settings;
      return patchVision(settings, { vr_enabled: !settings.vision.vr_enabled });
    }
    case "toggle_ocr":
      return patchOcr(settings, { enabled: !settings.ocr.enabled });
    case "toggle_ocr_gesture":
      return patchOcr(settings, { hand_gesture_enabled: !settings.ocr.hand_gesture_enabled });
    case "preview_headset":
    case "toggle_capture":
    case "toggle_network_capture":
    case "preview_wrist":
    case "open_ocr_bindings":
      return settings;
  }
}
