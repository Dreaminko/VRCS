import { SlidersHorizontal } from "lucide-react";
import { useTranslation } from "react-i18next";

import { TranslationRouteList } from "../../settings/translation/TranslationRouteList";
import type {
  ApiProfileView,
  CaptureStartInput,
  LanguageOverrideInput,
  Settings,
} from "../../types";

const RECOGNITION_LANGUAGES: Settings["asr"]["language"][] = [
  "auto",
  "en",
  "ja",
  "zh",
  "ko",
  "es",
  "fr",
  "de",
];

export function LanguageSessionControl({
  settings,
  apiProfiles,
  running,
  value,
  onChange,
}: {
  settings: Settings | null;
  apiProfiles: ApiProfileView[];
  running: boolean;
  value: CaptureStartInput;
  onChange: (value: CaptureStartInput) => void;
}) {
  const { t } = useTranslation();
  const selection = value.language_override
    ? "temporary"
    : value.language_preset_id ?? "global";
  const updateOverride = (patch: Partial<LanguageOverrideInput>) => {
    if (!value.language_override) return;
    onChange({ language_override: { ...value.language_override, ...patch } });
  };

  return (
    <div className={`language-session-control ${selection === "temporary" ? "expanded" : ""}`}>
      <label className="language-session-select">
        <span>{t("settings.translation.session")}</span>
        <select
          value={selection}
          disabled={running || !settings}
          onChange={(event) => {
            const next = event.target.value;
            if (next === "global") onChange({});
            else if (next === "temporary" && settings) {
              onChange({ language_override: overrideFromSettings(settings) });
            } else onChange({ language_preset_id: next });
          }}
        >
          <option value="global">{t("settings.translation.globalSession")}</option>
          {settings?.language_presets.map((preset) => (
            <option value={preset.id} key={preset.id}>{preset.name}</option>
          ))}
          <option value="temporary">{t("settings.translation.temporarySession")}</option>
        </select>
      </label>

      {settings && value.language_override && (
        <div className="language-session-panel">
          <header>
            <SlidersHorizontal size={15} />
            <strong>{t("settings.translation.sessionOverrides")}</strong>
            <small>{t("settings.translation.sessionOverridesHint")}</small>
          </header>
          <div className="language-session-basics">
            <label>
              <span>{t("settings.recognition.language")}</span>
              <select
                value={value.language_override.recognition_language}
                disabled={running}
                onChange={(event) => updateOverride({
                  recognition_language: event.target.value as LanguageOverrideInput["recognition_language"],
                })}
              >
                {RECOGNITION_LANGUAGES.map((language) => (
                  <option value={language} key={language}>{language.toUpperCase()}</option>
                ))}
              </select>
            </label>
            <label>
              <span>{t("settings.translation.mode")}</span>
              <select
                value={value.language_override.translation_mode}
                disabled={running}
                onChange={(event) => updateOverride({
                  translation_mode: event.target.value as LanguageOverrideInput["translation_mode"],
                })}
              >
                {(["disabled", "manual", "automatic"] as const).map((mode) => (
                  <option value={mode} key={mode}>{t(`settings.translation.modes.${mode}`)}</option>
                ))}
              </select>
            </label>
            <label>
              <span>{t("settings.translation.oscStrategy")}</span>
              <select
                value={value.language_override.osc_translation_strategy}
                disabled={running}
                onChange={(event) => updateOverride({
                  osc_translation_strategy: event.target.value as LanguageOverrideInput["osc_translation_strategy"],
                })}
              >
                <option value="preferred_only">{t("settings.translation.oscStrategies.preferredOnly")}</option>
                <option value="round_robin">{t("settings.translation.oscStrategies.roundRobin")}</option>
              </select>
            </label>
          </div>
          <TranslationRouteList
            title={t("settings.translation.targetLanguageForSelf")}
            targets={value.language_override.microphone_targets}
            profiles={apiProfiles}
            disabled={running}
            onChange={(microphone_targets) => updateOverride({ microphone_targets })}
          />
          <TranslationRouteList
            title={t("settings.translation.targetLanguageForOtherParty")}
            targets={value.language_override.speaker_targets}
            profiles={apiProfiles}
            disabled={running}
            onChange={(speaker_targets) => updateOverride({ speaker_targets })}
          />
        </div>
      )}
    </div>
  );
}

function overrideFromSettings(settings: Settings): LanguageOverrideInput {
  return {
    recognition_language: settings.asr.language,
    translation_mode: settings.translation.mode,
    speaker_targets: structuredClone(settings.translation.speaker_targets),
    microphone_targets: structuredClone(settings.translation.microphone_targets),
    osc_translation_strategy: settings.osc.translation_strategy,
  };
}
