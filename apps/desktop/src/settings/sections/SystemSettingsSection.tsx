import { ChevronDown, ChevronUp, Settings2, SlidersHorizontal } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { DesktopPreferences } from "../../desktop-preferences";
import { localeCatalog } from "../../i18n/catalog";
import {
  INTERFACE_SCALE_MAX,
  INTERFACE_SCALE_MIN,
  INTERFACE_SCALE_STEP,
  normalizeInterfaceScale,
} from "../../app/interface-scale";
import {
  readTranscriptionStartBehavior,
  writeTranscriptionStartBehavior,
  type TranscriptionStartBehavior,
} from "../../transcription-start";
import type { Settings } from "../../types";
import type { UiLanguagePreference } from "../../app/ui-language";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle, Select } from "../SettingsControls";
import { StorageSettingsSection } from "./StorageSettingsSection";

export function SystemSettingsSection({
  desktopPreferences,
  desktopPreferencesReady,
  desktopSaveState,
  uiLanguagePreference,
  interfaceScale,
  onUpdateDesktop,
  onInterfaceScaleChange,
  onUpdateUiLanguage,
  onboardingDisabled,
  onStartOnboarding,
  locale,
  draft,
  saveState,
  applySettings,
}: {
  desktopPreferences: DesktopPreferences;
  desktopPreferencesReady: boolean;
  desktopSaveState: SaveState;
  uiLanguagePreference: UiLanguagePreference;
  interfaceScale: number;
  onUpdateDesktop: (key: keyof DesktopPreferences, enabled: boolean) => Promise<void>;
  onInterfaceScaleChange: (value: number) => void;
  onUpdateUiLanguage: (preference: UiLanguagePreference) => Promise<void>;
  onboardingDisabled: boolean;
  onStartOnboarding: () => void;
  locale: string;
  draft: Settings;
  saveState: SaveState;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const [transcriptionStartBehavior, setTranscriptionStartBehavior] = useState(
    readTranscriptionStartBehavior,
  );
  const [interfaceScaleText, setInterfaceScaleText] = useState(() => String(interfaceScale));

  useEffect(() => {
    setInterfaceScaleText(String(interfaceScale));
  }, [interfaceScale]);

  const commitInterfaceScale = () => {
    if (!interfaceScaleText.trim() || !Number.isFinite(Number(interfaceScaleText))) {
      setInterfaceScaleText(String(interfaceScale));
      return;
    }
    const next = normalizeInterfaceScale(interfaceScaleText);
    setInterfaceScaleText(String(next));
    if (next !== interfaceScale) onInterfaceScaleChange(next);
  };

  const stepInterfaceScale = (direction: -1 | 1) => {
    const next = normalizeInterfaceScale(interfaceScale + direction * INTERFACE_SCALE_STEP);
    setInterfaceScaleText(String(next));
    if (next !== interfaceScale) onInterfaceScaleChange(next);
  };

  return (
    <div className="settings-section settings-section-active system-section" id="settings-panel-system" role="tabpanel" aria-labelledby="settings-tab-system">
      <div className="section-heading system-page-heading">
        <div><SlidersHorizontal size={18} /><h2>{t("settings.system.title")}</h2></div>
      </div>
      <div className="system-settings-list">
        <section className="system-settings-group" aria-labelledby="system-general-title">
          <div className="section-heading">
            <div><Settings2 size={18} /><h3 id="system-general-title">{t("settings.system.general")}</h3></div>
          </div>
          <div className="system-select-setting">
            <div>
              <strong>{t("settings.system.language")}</strong>
            </div>
            <Select
              label={t("settings.system.language")}
              value={uiLanguagePreference}
              options={[
                { value: "system", label: t("settings.system.followSystem") },
                ...localeCatalog.map(({ _meta }) => ({
                  value: _meta.locale,
                  label: _meta.name,
                })),
              ]}
              disabled={desktopSaveState === "saving"}
              hideLabel
              onChange={(value) => void onUpdateUiLanguage(value as UiLanguagePreference)}
            />
          </div>
          <div className="system-select-setting">
            <div>
              <strong>{t("settings.system.transcriptionStartBehavior")}</strong>
            </div>
            <Select
              label={t("settings.system.transcriptionStartBehavior")}
              value={transcriptionStartBehavior}
              options={[
                {
                  value: "continue_current",
                  label: t("settings.system.continueCurrentConversation"),
                },
                {
                  value: "new_conversation",
                  label: t("settings.system.createNewConversation"),
                },
              ]}
              disabled={false}
              hideLabel
              onChange={(value) => {
                const behavior = value as TranscriptionStartBehavior;
                setTranscriptionStartBehavior(behavior);
                writeTranscriptionStartBehavior(behavior);
              }}
            />
          </div>
          <div className="system-scale-setting">
            <div>
              <strong>{t("settings.system.interfaceScale")}</strong>
            </div>
            <div className="system-scale-input" role="group" aria-label={t("settings.system.interfaceScale")}>
              <span className="system-scale-value">
                <input
                  type="number"
                  inputMode="numeric"
                  min={INTERFACE_SCALE_MIN}
                  max={INTERFACE_SCALE_MAX}
                  step={INTERFACE_SCALE_STEP}
                  value={interfaceScaleText}
                  aria-label={t("settings.system.interfaceScale")}
                  onChange={(event) => {
                    const text = event.currentTarget.value;
                    const value = event.currentTarget.valueAsNumber;
                    setInterfaceScaleText(text);
                    if (
                      Number.isFinite(value)
                      && value >= INTERFACE_SCALE_MIN
                      && value <= INTERFACE_SCALE_MAX
                      && value % INTERFACE_SCALE_STEP === 0
                    ) {
                      onInterfaceScaleChange(value);
                    }
                  }}
                  onBlur={commitInterfaceScale}
                  onKeyDown={(event) => {
                    if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                      event.preventDefault();
                      stepInterfaceScale(event.key === "ArrowUp" ? 1 : -1);
                    }
                    if (event.key === "Enter") event.currentTarget.blur();
                  }}
                />
                <em aria-hidden="true">%</em>
              </span>
              <span className="system-scale-stepper">
                <button
                  type="button"
                  disabled={interfaceScale >= INTERFACE_SCALE_MAX}
                  aria-label={`${t("settings.system.interfaceScale")} +${INTERFACE_SCALE_STEP}%`}
                  title={`+${INTERFACE_SCALE_STEP}%`}
                  onClick={() => stepInterfaceScale(1)}
                >
                  <ChevronUp size={13} strokeWidth={2} />
                </button>
                <button
                  type="button"
                  disabled={interfaceScale <= INTERFACE_SCALE_MIN}
                  aria-label={`${t("settings.system.interfaceScale")} -${INTERFACE_SCALE_STEP}%`}
                  title={`-${INTERFACE_SCALE_STEP}%`}
                  onClick={() => stepInterfaceScale(-1)}
                >
                  <ChevronDown size={13} strokeWidth={2} />
                </button>
              </span>
            </div>
          </div>
          <div className="settings-toggle-list">
            <PreferenceToggle
              title={t("settings.system.launchAtStartup")}
              checked={desktopPreferences.launchAtStartup}
              disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
              onChange={(enabled) => void onUpdateDesktop("launchAtStartup", enabled)}
            />
            <PreferenceToggle
              title={t("settings.system.minimizeToTray")}
              checked={desktopPreferences.minimizeToTray}
              disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
              onChange={(enabled) => void onUpdateDesktop("minimizeToTray", enabled)}
            />
          </div>
          <div className="system-onboarding-setting">
            <div>
              <strong>{t("settings.system.runOnboarding")}</strong>
            </div>
            <button className="secondary-button" type="button" disabled={onboardingDisabled} onClick={onStartOnboarding}>
              {t(onboardingDisabled ? "settings.system.runOnboardingStopFirst" : "settings.system.runOnboardingAction")}
            </button>
          </div>
        </section>
        <StorageSettingsSection
          locale={locale}
          draft={draft}
          saveState={saveState}
          applySettings={applySettings}
        />
      </div>
    </div>
  );
}
