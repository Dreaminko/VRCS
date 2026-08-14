import { RotateCcw, SlidersHorizontal } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import type { DesktopPreferences } from "../../desktop-preferences";
import { localeCatalog } from "../../i18n/catalog";
import {
  INTERFACE_SCALE_MAX,
  INTERFACE_SCALE_MIN,
  INTERFACE_SCALE_STEP,
} from "../../interface-scale";
import {
  readTranscriptionStartBehavior,
  writeTranscriptionStartBehavior,
  type TranscriptionStartBehavior,
} from "../../transcription-start";
import type { Settings } from "../../types";
import type { UiLanguagePreference } from "../../ui-language";
import type { ApplySettings, SaveState } from "../settings-types";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";
import { ExternalApiSettingsCard } from "../system/ExternalApiSettingsCard";

export function SystemSettingsSection({
  draft,
  coreSaveState,
  applySettings,
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
}: {
  draft: Settings;
  coreSaveState: SaveState;
  applySettings: ApplySettings;
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
}) {
  const { t } = useTranslation();
  const [transcriptionStartBehavior, setTranscriptionStartBehavior] = useState(
    readTranscriptionStartBehavior,
  );
  const updateExternalApi = (patch: Partial<Settings["external_api"]>) => {
    applySettings((current) => ({
      ...current,
      external_api: { ...current.external_api, ...patch },
    }));
  };

  return (
    <div className="settings-section settings-section-active system-section" id="settings-panel-system" role="tabpanel" aria-labelledby="settings-tab-system">
      <div className="section-heading">
        <div><SlidersHorizontal size={18} /><h2>{t("settings.system.title")}</h2><span>{t("settings.system.subtitle")}</span></div>
        <p>{t("settings.system.saveImmediately")}</p>
      </div>
      <div className="system-select-setting">
        <div>
          <strong>{t("settings.system.language")}</strong>
          <small>{t("settings.system.languageDescription")}</small>
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
          onChange={(value) => void onUpdateUiLanguage(value as UiLanguagePreference)}
        />
      </div>
      <div className="system-select-setting">
        <div>
          <strong>{t("settings.system.transcriptionStartBehavior")}</strong>
          <small>{t("settings.system.transcriptionStartBehaviorDescription")}</small>
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
          onChange={(value) => {
            const behavior = value as TranscriptionStartBehavior;
            setTranscriptionStartBehavior(behavior);
            writeTranscriptionStartBehavior(behavior);
          }}
        />
      </div>
      <div className="system-scale-setting">
        <RangeField
          label={t("settings.system.interfaceScale")}
          helper={t("settings.system.interfaceScaleDescription")}
          value={interfaceScale}
          min={INTERFACE_SCALE_MIN}
          max={INTERFACE_SCALE_MAX}
          step={INTERFACE_SCALE_STEP}
          disabled={false}
          formatValue={(value) => `${value}%`}
          onCommit={onInterfaceScaleChange}
        />
      </div>
      <div className="system-onboarding-setting">
        <div>
          <strong>{t("settings.system.runOnboarding")}</strong>
          <small>{t("settings.system.runOnboardingDescription")}</small>
        </div>
        <button className="secondary-button" type="button" disabled={onboardingDisabled} onClick={onStartOnboarding}>
          <RotateCcw size={15} />
          {t(onboardingDisabled ? "settings.system.runOnboardingStopFirst" : "settings.system.runOnboardingAction")}
        </button>
      </div>
      <div className="settings-toggle-list">
        <PreferenceToggle
          title={t("settings.system.launchAtStartup")}
          description={t("settings.system.launchAtStartupDescription")}
          checked={desktopPreferences.launchAtStartup}
          disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
          onChange={(enabled) => void onUpdateDesktop("launchAtStartup", enabled)}
        />
        <PreferenceToggle
          title={t("settings.system.minimizeToTray")}
          description={t("settings.system.minimizeToTrayDescription")}
          checked={desktopPreferences.minimizeToTray}
          disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
          onChange={(enabled) => void onUpdateDesktop("minimizeToTray", enabled)}
        />
      </div>
      <ExternalApiSettingsCard
        config={draft.external_api}
        saveState={coreSaveState}
        onChange={updateExternalApi}
      />
    </div>
  );
}
