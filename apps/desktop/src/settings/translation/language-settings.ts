import type { ApiProfileView } from "../../providers/types";
import { liveTranslationServiceName } from "../../recognition-services.ts";
import { TRANSLATION_LANGUAGE_CODES } from "../../translation-languages.ts";
import type { Settings, TranslationTargetSettings } from "../types";

export function targetLanguageCodes(target: TranslationTargetSettings, usedLanguages: string[], profiles: ApiProfileView[], liveService?: string): string[] {
  const native = liveTranslationServiceName(liveService);
  const profile = profiles.find((item) => item.id === target.profile_id);
  const codes = native
    ? TRANSLATION_LANGUAGE_CODES.filter((code) => liveService !== "gemini_live_translate" || !["yue-Hant", "nl"].includes(code))
    : profile?.capabilities.supported_languages ?? TRANSLATION_LANGUAGE_CODES;
  return codes.filter((code) => code === target.target_language || !usedLanguages.includes(code));
}

export function saveLanguagePreset(settings: Settings, name: string): Settings {
  if (settings.language_presets.length >= 5) return settings;
  return {
    ...settings,
    language_presets: [...settings.language_presets, {
      id: crypto.randomUUID(), name,
      recognition_language: settings.asr.language,
      translation_mode: settings.translation.mode,
      speaker_targets: structuredClone(settings.translation.speaker_targets),
      microphone_targets: structuredClone(settings.translation.microphone_targets),
      osc_translation_strategy: settings.osc.translation_strategy,
    }],
  };
}

export function applyLanguagePreset(settings: Settings, id: string): Settings {
  const preset = settings.language_presets.find((item) => item.id === id);
  if (!preset) return settings;
  return {
    ...settings,
    asr: { ...settings.asr, language: preset.recognition_language },
    translation: { ...settings.translation, mode: preset.translation_mode,
      speaker_targets: structuredClone(preset.speaker_targets),
      microphone_targets: structuredClone(preset.microphone_targets) },
    osc: { ...settings.osc, translation_strategy: preset.osc_translation_strategy },
  };
}
