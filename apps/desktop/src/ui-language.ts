import { isTauri } from "@tauri-apps/api/core";

export const SUPPORTED_UI_LOCALES = ["zh-CN", "ja-JP", "en-US"] as const;
export type UiLocale = (typeof SUPPORTED_UI_LOCALES)[number];
export type UiLanguagePreference = "system" | UiLocale;

const UI_LANGUAGE_KEY = "uiLanguage";
const WEB_STORAGE_KEY = "vrcs.ui-language";

export function isUiLanguagePreference(value: unknown): value is UiLanguagePreference {
  return value === "system"
    || SUPPORTED_UI_LOCALES.includes(value as UiLocale);
}

export function resolveUiLocale(
  preference: UiLanguagePreference,
  systemLanguages: readonly string[] = typeof navigator === "undefined"
    ? []
    : navigator.languages,
): UiLocale {
  if (preference !== "system") return preference;

  for (const language of systemLanguages) {
    const normalized = language.toLowerCase();
    if (normalized === "zh" || normalized.startsWith("zh-")) return "zh-CN";
    if (normalized === "ja" || normalized.startsWith("ja-")) return "ja-JP";
    if (normalized === "en" || normalized.startsWith("en-")) return "en-US";
  }
  return "en-US";
}

export async function loadUiLanguagePreference(): Promise<UiLanguagePreference> {
  if (!isTauri()) {
    const stored = globalThis.localStorage?.getItem(WEB_STORAGE_KEY);
    return isUiLanguagePreference(stored) ? stored : "system";
  }

  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  const stored = await store.get(UI_LANGUAGE_KEY);
  return isUiLanguagePreference(stored) ? stored : "system";
}

export async function saveUiLanguagePreference(
  preference: UiLanguagePreference,
): Promise<void> {
  if (!isTauri()) {
    globalThis.localStorage?.setItem(WEB_STORAGE_KEY, preference);
    return;
  }

  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  await store.set(UI_LANGUAGE_KEY, preference);
  await store.save();
}
