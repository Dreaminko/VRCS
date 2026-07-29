import { isTauri } from "@tauri-apps/api/core";

export type UiLocale = string;
export type UiLanguagePreference = "system" | UiLocale;

const UI_LANGUAGE_KEY = "uiLanguage";
const WEB_STORAGE_KEY = "vrcs.ui-language";

export function isUiLanguagePreference(
  value: unknown,
  supportedLocales: readonly string[],
): value is UiLanguagePreference {
  return value === "system"
    || (typeof value === "string" && supportedLocales.includes(value));
}

export function resolveUiLocale(
  preference: UiLanguagePreference,
  supportedLocales: readonly string[],
  systemLanguages: readonly string[] = typeof navigator === "undefined" ? [] : navigator.languages,
  fallbackLocale = "en-US",
): UiLocale {
  if (preference !== "system" && supportedLocales.includes(preference)) return preference;

  const normalizedLocales = supportedLocales.map((locale) => ({
    locale,
    normalized: locale.toLowerCase(),
    language: locale.split("-")[0].toLowerCase(),
  }));
  for (const language of systemLanguages) {
    const normalized = language.toLowerCase();
    const exact = normalizedLocales.find((candidate) => candidate.normalized === normalized);
    if (exact) return exact.locale;
    const languageCode = normalized.split("-")[0];
    const sameLanguage = normalizedLocales.find(
      (candidate) => candidate.language === languageCode,
    );
    if (sameLanguage) return sameLanguage.locale;
  }
  return supportedLocales.includes(fallbackLocale)
    ? fallbackLocale
    : supportedLocales[0] ?? fallbackLocale;
}

export async function loadUiLanguagePreference(
  supportedLocales: readonly string[],
): Promise<UiLanguagePreference> {
  if (!isTauri()) {
    const stored = globalThis.localStorage?.getItem(WEB_STORAGE_KEY);
    return isUiLanguagePreference(stored, supportedLocales) ? stored : "system";
  }

  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  const stored = await store.get(UI_LANGUAGE_KEY);
  return isUiLanguagePreference(stored, supportedLocales) ? stored : "system";
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
