import { BookmarkPlus, Play, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { LanguagePreset, Settings } from "../../types";

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
    <section className="translation-preset-group">
      <header className="translation-route-group-header">
        <div>
          <strong>{t("settings.translation.presets")}</strong>
          <small>{t("settings.translation.presetsHint")}</small>
        </div>
      </header>
      <div className="translation-preset-settings">
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
    </section>
  );
}
