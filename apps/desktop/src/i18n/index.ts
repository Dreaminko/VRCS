import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { invoke, isTauri } from "@tauri-apps/api/core";
import enUS from "./locales/en-US.json";
import jaJP from "./locales/ja-JP.json";
import zhCN from "./locales/zh-CN.json";
import {
  loadUiLanguagePreference,
  resolveUiLocale,
  saveUiLanguagePreference,
} from "../ui-language";
import type { UiLanguagePreference, UiLocale } from "../ui-language";

let selectedPreference: UiLanguagePreference = "system";

function applyDocumentLocale(locale: UiLocale): void {
  document.documentElement.lang = locale;
  document.documentElement.dir = "ltr";
}

async function syncNativeLabels(): Promise<void> {
  if (!isTauri()) return;
  await invoke("update_native_labels", {
    show: i18n.t("native.tray.show"),
    quit: i18n.t("native.tray.quit"),
  });
}

export async function initializeI18n(): Promise<void> {
  selectedPreference = await loadUiLanguagePreference();
  const locale = resolveUiLocale(selectedPreference);

  await i18n
    .use(initReactI18next)
    .init({
      resources: {
        "en-US": { translation: enUS.translation },
        "ja-JP": { translation: jaJP.translation },
        "zh-CN": { translation: zhCN.translation },
      },
      lng: locale,
      fallbackLng: "en-US",
      supportedLngs: ["en-US", "ja-JP", "zh-CN"],
      load: "currentOnly",
      returnEmptyString: false,
      interpolation: { escapeValue: false },
      react: { useSuspense: false },
    });

  applyDocumentLocale(locale);
  await syncNativeLabels().catch(() => undefined);
}

export function currentUiLanguagePreference(): UiLanguagePreference {
  return selectedPreference;
}

export async function changeUiLanguage(
  preference: UiLanguagePreference,
): Promise<void> {
  const previous = selectedPreference;
  selectedPreference = preference;
  const locale = resolveUiLocale(preference);

  try {
    await i18n.changeLanguage(locale);
    applyDocumentLocale(locale);
    await syncNativeLabels().catch(() => undefined);
    await saveUiLanguagePreference(preference);
  } catch (error) {
    selectedPreference = previous;
    await i18n.changeLanguage(resolveUiLocale(previous));
    applyDocumentLocale(resolveUiLocale(previous));
    await syncNativeLabels().catch(() => undefined);
    throw error;
  }
}

export default i18n;
