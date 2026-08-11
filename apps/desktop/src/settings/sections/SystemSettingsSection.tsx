import { useState } from "react";
import { useTranslation } from "react-i18next";
import { SlidersHorizontal } from "lucide-react";

import type { DesktopPreferences } from "../../desktop-preferences";
import { localeCatalog } from "../../i18n/catalog";
import {
  INTERFACE_SCALE_MAX,
  INTERFACE_SCALE_MIN,
  INTERFACE_SCALE_STEP,
} from "../../interface-scale";
import type { UiLanguagePreference } from "../../ui-language";
import {
  readTranscriptionStartBehavior,
  writeTranscriptionStartBehavior,
  type TranscriptionStartBehavior,
} from "../../transcription-start";
import type { SaveState } from "../settings-types";
import { PreferenceToggle, RangeField, Select } from "../SettingsControls";

export function SystemSettingsSection({
  desktopPreferences,
  desktopPreferencesReady,
  desktopSaveState,
  uiLanguagePreference,
  interfaceScale,
  onUpdateDesktop,
  onInterfaceScaleChange,
  onUpdateUiLanguage,
}: {
  desktopPreferences: DesktopPreferences;
  desktopPreferencesReady: boolean;
  desktopSaveState: SaveState;
  uiLanguagePreference: UiLanguagePreference;
  interfaceScale: number;
  onUpdateDesktop: (key: keyof DesktopPreferences, enabled: boolean) => Promise<void>;
  onInterfaceScaleChange: (value: number) => void;
  onUpdateUiLanguage: (preference: UiLanguagePreference) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [transcriptionStartBehavior, setTranscriptionStartBehavior] = useState(
    readTranscriptionStartBehavior,
  );
  const updateDesktop = onUpdateDesktop;
  const updateUiLanguage = onUpdateUiLanguage;
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
              onChange={(value) => void updateUiLanguage(value as UiLanguagePreference)}
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
          <div className="settings-toggle-list">
            <PreferenceToggle
              title={t("settings.system.launchAtStartup")}
              description={t("settings.system.launchAtStartupDescription")}
              checked={desktopPreferences.launchAtStartup}
              disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
              onChange={(enabled) => void updateDesktop("launchAtStartup", enabled)}
            />
            <PreferenceToggle
              title={t("settings.system.minimizeToTray")}
              description={t("settings.system.minimizeToTrayDescription")}
              checked={desktopPreferences.minimizeToTray}
              disabled={!desktopPreferencesReady || desktopSaveState === "saving"}
              onChange={(enabled) => void updateDesktop("minimizeToTray", enabled)}
            />
          </div>
        </div>
  );
}
