import { useEffect, useState } from "react";
import { BookmarkPlus, Play, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type {
  LanguagePreset,
  Settings,
} from "../types";
import { applyLanguagePreset, saveLanguagePreset } from "./language-settings";

function PresetNameInput({ name, disabled, onCommit }: {
  name: string;
  disabled: boolean;
  onCommit: (name: string) => void;
}) {
  const { t } = useTranslation();
  const [draftName, setDraftName] = useState(name);

  useEffect(() => setDraftName(name), [name]);

  return (
    <input
      aria-label={t("settings.translation.presetName")}
      maxLength={40}
      value={draftName}
      disabled={disabled}
      onChange={(event) => setDraftName(event.target.value)}
      onBlur={() => {
        if (!draftName.trim()) {
          setDraftName(name);
          return;
        }
        if (draftName !== name) onCommit(draftName);
      }}
    />
  );
}

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
    onChange(saveLanguagePreset(settings, t("settings.translation.presetDefaultName", {
      count: settings.language_presets.length + 1,
    })));
  };
  const updatePreset = (index: number, patch: Partial<LanguagePreset>) => {
    const language_presets = [...settings.language_presets];
    language_presets[index] = { ...language_presets[index], ...patch };
    onChange({ ...settings, language_presets });
  };
  const applyPreset = (preset: LanguagePreset) => onChange(applyLanguagePreset(settings, preset.id));

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
              <PresetNameInput
                name={preset.name}
                disabled={disabled}
                onCommit={(name) => updatePreset(index, { name })}
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
