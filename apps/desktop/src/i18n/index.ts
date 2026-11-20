import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { invoke, isTauri } from "@tauri-apps/api/core";
import * as catalog from "./catalog";
import {
  loadUiLanguagePreference,
  resolveUiLocale,
  saveUiLanguagePreference,
} from "../app/ui-language";
import type { UiLanguagePreference, UiLocale } from "../app/ui-language";

let selectedPreference: UiLanguagePreference = "system";

async function loadLocaleTranslation(locale: string): Promise<Record<string, unknown>> {
  const loadLocaleResource = Reflect.get(catalog, "loadLocaleResource") as
    | ((targetLocale: string) => Promise<{ translation: Record<string, unknown> }>)
    | undefined;
  if (typeof loadLocaleResource === "function") {
    return (await loadLocaleResource(locale)).translation;
  }
  const bundled = Reflect.get(catalog, "localeResources") as
    | Record<string, { translation: Record<string, unknown> }>
    | undefined;
  const translation = bundled?.[locale]?.translation;
  if (!translation) throw new Error(`Unsupported locale: ${locale}`);
  return translation;
}

function applyDocumentLocale(locale: UiLocale): void {
  document.documentElement.lang = locale;
  document.documentElement.dir = catalog.localeCatalog.find(
    (resource) => resource._meta.locale === locale,
  )?._meta.direction ?? "ltr";
}

async function syncNativeLabels(): Promise<void> {
  if (!isTauri()) return;
  await invoke("update_native_labels", {
    show: i18n.t("native.tray.show"),
    quit: i18n.t("native.tray.quit"),
  });
}

export async function initializeI18n({ nativePreferences = true }: { nativePreferences?: boolean } = {}): Promise<void> {
  selectedPreference = nativePreferences ? await loadUiLanguagePreference(catalog.supportedUiLocales) : "system";
  const locale = resolveUiLocale(selectedPreference, catalog.supportedUiLocales);
  const localesToLoad = locale === "en-US" ? ["en-US"] : [locale, "en-US"];
  const loaded = await Promise.all(
    localesToLoad.map(async (code) => [code, { translation: await loadLocaleTranslation(code) }] as const),
  );

  await i18n
    .use(initReactI18next)
    .init({
      resources: Object.fromEntries(loaded),
      lng: locale,
      fallbackLng: "en-US",
      supportedLngs: catalog.supportedUiLocales,
      load: "currentOnly",
      returnEmptyString: false,
      interpolation: { escapeValue: false },
      react: { useSuspense: false },
    });

  applyDocumentLocale(locale);
  if (nativePreferences) void syncNativeLabels().catch(() => undefined);
}

export function currentUiLanguagePreference(): UiLanguagePreference {
  return selectedPreference;
}

export async function changeUiLanguage(
  preference: UiLanguagePreference,
): Promise<void> {
  const previous = selectedPreference;
  selectedPreference = preference;
  const locale = resolveUiLocale(preference, catalog.supportedUiLocales);

  try {
    if (!i18n.hasResourceBundle(locale, "translation")) {
      i18n.addResourceBundle(locale, "translation", await loadLocaleTranslation(locale), true, true);
    }
    await i18n.changeLanguage(locale);
    applyDocumentLocale(locale);
    await syncNativeLabels().catch(() => undefined);
    await saveUiLanguagePreference(preference);
  } catch (error) {
    selectedPreference = previous;
    const previousLocale = resolveUiLocale(previous, catalog.supportedUiLocales);
    if (!i18n.hasResourceBundle(previousLocale, "translation")) {
      i18n.addResourceBundle(
        previousLocale,
        "translation",
        await loadLocaleTranslation(previousLocale).catch(() => ({})),
        true,
        true,
      );
    }
    await i18n.changeLanguage(previousLocale);
    applyDocumentLocale(previousLocale);
    await syncNativeLabels().catch(() => undefined);
    throw error;
  }
}

export default i18n;
