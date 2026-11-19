import type { FeatureKey, FeatureSettings } from "./types";

export type FeaturePresetId = "translation_only" | "full";
export type FeaturePresetMode = FeaturePresetId | "custom";

export const FEATURE_KEYS: readonly FeatureKey[] = [
  "glossary",
  "learning",
  "anki",
  "ocr",
  "visual_question_answering",
  "vr_overlay",
  "external_api",
  "vrcx",
];

export const FEATURE_PRESET_MODES: readonly FeaturePresetMode[] = [
  "translation_only",
  "full",
  "custom",
];

type TranslationOnlyManagedKey = Exclude<FeatureKey, "vr_overlay">;

const TRANSLATION_ONLY_VALUES: Record<TranslationOnlyManagedKey, boolean> = {
  glossary: true,
  learning: false,
  anki: false,
  vrcx: false,
  ocr: false,
  visual_question_answering: false,
  external_api: false,
};

const TRANSLATION_ONLY_KEYS = Object.keys(TRANSLATION_ONLY_VALUES) as TranslationOnlyManagedKey[];

const FULL_FEATURE_VALUES: FeatureSettings = {
  glossary: true,
  learning: true,
  anki: true,
  vrcx: true,
  ocr: true,
  visual_question_answering: true,
  vr_overlay: true,
  external_api: true,
};

export function applyFeaturePreset(
  features: FeatureSettings,
  preset: FeaturePresetId,
): FeatureSettings {
  if (preset === "full") {
    return { ...FULL_FEATURE_VALUES };
  }
  return {
    ...features,
    ...TRANSLATION_ONLY_VALUES,
    vr_overlay: features.vr_overlay,
  };
}

export function matchFeaturePreset(features: FeatureSettings): FeaturePresetId | null {
  if (FEATURE_KEYS.every((key) => features[key] === FULL_FEATURE_VALUES[key])) {
    return "full";
  }
  if (TRANSLATION_ONLY_KEYS.every((key) => features[key] === TRANSLATION_ONLY_VALUES[key])) {
    return "translation_only";
  }
  return null;
}

export function areFeatureSettingsEqual(a: FeatureSettings, b: FeatureSettings): boolean {
  return FEATURE_KEYS.every((key) => a[key] === b[key]);
}
