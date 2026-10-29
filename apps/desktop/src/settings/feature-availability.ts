import type { SettingsCategory } from "./settings-types";
import type { FeatureSettings } from "./types";

export const DEFAULT_FEATURE_SETTINGS: FeatureSettings = {
  glossary: true, learning: true, anki: true, microphone_speaker_filter: false,
  vrcx: true, ocr: true, vr_overlay: true, external_api: true, visual_question_answering: false,
};

export function visibleSettingsCategories(features: FeatureSettings): SettingsCategory[] {
  const categories: SettingsCategory[] = ["system", "audio", "recognition", "translation"];
  if (features.vr_overlay) categories.push("vr_overlay");
  if (features.ocr || features.visual_question_answering) categories.push("vision");
  if (features.glossary) categories.push("glossary");
  categories.push("api");
  if (features.learning) categories.push("learning");
  categories.push("connections");
  categories.push("debug");
  return categories;
}

export function resolveSettingsCategory(requested: SettingsCategory, features: FeatureSettings): SettingsCategory {
  const category = requested === "ocr" ? "vision" : requested;
  return visibleSettingsCategories(features).includes(category) ? category : "system";
}
