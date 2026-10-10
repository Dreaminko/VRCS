import type { SettingsCategory } from "./settings-types";
import type { FeatureSettings } from "./types";

export const DEFAULT_FEATURE_SETTINGS: FeatureSettings = {
  glossary: true, learning: true, anki: true, osc_chatbox: true,
  vrcx: true, ocr: true, vr_overlay: true, external_api: true,
};

export function visibleSettingsCategories(features: FeatureSettings): SettingsCategory[] {
  const categories: SettingsCategory[] = ["system", "audio", "recognition", "translation"];
  if (features.glossary) categories.push("glossary");
  categories.push("api");
  if (features.learning) categories.push("learning");
  if (features.anki || features.osc_chatbox || features.vrcx || features.external_api) categories.push("connections");
  if (features.ocr) categories.push("ocr");
  if (features.vr_overlay) categories.push("vr_overlay");
  categories.push("debug");
  return categories;
}

export function resolveSettingsCategory(requested: SettingsCategory, features: FeatureSettings): SettingsCategory {
  return visibleSettingsCategories(features).includes(requested) ? requested : "system";
}
