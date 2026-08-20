import { BookmarkPlus, Play, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { LanguagePreset, Settings } from "../../types";
import { Select } from "../SettingsControls";

export function LanguagePresetSettings({
  settings,
  disabled,
  onChange,
}: {
  settings: Settings;
  disabled: boolean;
  onChange: (settings: Settings) => void;
}) {
  const { t } = useTranslation();
  const savePreset = () => {
    if (settings.language_presets.length >= 5) return;
    const preset: LanguagePreset = {
      id: crypto.randomUUID(),
      name: t("settings.translation.presetDefaultName", {
        count: settings.language_presets.length + 1,
      }),
      recognition_language: settings.asr.language,
      translation_mode: settings.translation.mode,
      speaker_targets: structuredClone(settings.translation.speaker_targets),
      microphone_targets: structuredClone(settings.translation.microphone_targets),
      osc_translation_strategy: settings.osc.translation_strategy,
    };
    onChange({ ...settings, language_presets: [...settings.language_presets, preset] });
  };
  const updatePreset = (index: number, patch: Partial<LanguagePreset>) => {
    const language_presets = [...settings.language_presets];
    language_presets[index] = { ...language_presets[index], ...patch };
    onChange({ ...settings, language_presets });
  };
  const applyPreset = (preset: LanguagePreset) => onChange({
    ...settings,
    asr: { ...settings.asr, language: preset.recognition_language },
    translation: {
      ...settings.translation,
      mode: preset.translation_mode,
      speaker_targets: structuredClone(preset.speaker_targets),
      microphone_targets: structuredClone(preset.microphone_targets),
    },
    osc: { ...settings.osc, translation_strategy: preset.osc_translation_strategy },
  });

  return (
    <div className="translation-config-row translation-presets-row">
      <div className="translation-config-title">
        <BookmarkPlus size={17} />
        <span>
          <strong>{t("settings.translation.presets")}</strong>
          <small>{t("settings.translation.presetsHint")}</small>
        </span>
      </div>
      <div className="translation-preset-settings">
        <Select
          label={t("settings.translation.oscStrategy")}
          value={settings.osc.translation_strategy}
          disabled={disabled}
          options={[
            { value: "preferred_only", label: t("settings.translation.oscStrategies.preferredOnly") },
            { value: "round_robin", label: t("settings.translation.oscStrategies.roundRobin") },
          ]}
          onChange={(translation_strategy) => onChange({
            ...settings,
            osc: {
              ...settings.osc,
              translation_strategy: translation_strategy as Settings["osc"]["translation_strategy"],
            },
          })}
        />
        <div className="translation-preset-list">
          {settings.language_presets.map((preset, index) => (
            <div className="translation-preset-row" key={preset.id}>
              <input
                aria-label={t("settings.translation.presetName")}
                maxLength={40}
                value={preset.name}
                disabled={disabled}
                onChange={(event) => {
                  if (event.target.value.trim()) updatePreset(index, { name: event.target.value });
                }}
              />
              <span>{preset.recognition_language} · {preset.speaker_targets.map((target) => target.target_language).join(" / ")}</span>
              <button type="button" aria-label={t("settings.translation.applyPreset")} disabled={disabled} onClick={() => applyPreset(preset)}><Play size={14} /></button>
              <button
                type="button"
                aria-label={t("common.delete")}
                disabled={disabled}
                onClick={() => onChange({
                  ...settings,
                  language_presets: settings.language_presets.filter((item) => item.id !== preset.id),
                })}
              ><Trash2 size={14} /></button>
            </div>
          ))}
        </div>
        <button className="secondary-button" type="button" disabled={disabled || settings.language_presets.length >= 5} onClick={savePreset}>
          <BookmarkPlus size={14} />
          {t("settings.translation.savePreset")}
        </button>
      </div>
    </div>
  );
}

export function swapSourceLanguages(settings: Settings, source: "speaker" | "microphone"): Settings {
  if (settings.asr.language === "auto") return settings;
  const field = source === "speaker" ? "speaker_targets" : "microphone_targets";
  const targets = [...settings.translation[field]];
  const preferred = targets[0];
  if (!preferred) return settings;
  const recognition = targetToRecognition(preferred.target_language);
  if (!recognition) return settings;
  targets[0] = { ...preferred, target_language: recognitionToTarget(settings.asr.language) };
  return {
    ...settings,
    asr: { ...settings.asr, language: recognition },
    translation: { ...settings.translation, [field]: targets },
  };
}

function targetToRecognition(language: string): Settings["asr"]["language"] | null {
  const base = language.toLowerCase().split("-")[0];
  if (base === "zh") return "zh";
  return ["en", "ja", "ko", "es", "fr", "de"].includes(base)
    ? base as Settings["asr"]["language"]
    : null;
}

function recognitionToTarget(language: Settings["asr"]["language"]): string {
  return language === "zh" ? "zh-Hans" : language;
}
