import { KeyRound } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ApiProfileView } from "../../providers/types";
import type {
  ApplySettings,
  RecognitionActions,
  RecognitionModels,
  SaveState,
} from "../settings-types";
import type { Settings } from "../types";
import { ApiManagementSettingsSection } from "./ApiManagementSettingsSection";
import { LocalModelsSection } from "./LocalModelsSection";
import { ModelAssignmentsSection } from "./ModelAssignmentsSection";

export type ApiSettingsTab = "models" | "profiles" | "local";

export function ApiSettingsSection({
  settings,
  profiles,
  applySettings,
  onRefreshSettings,
  activeTab,
  onTabChange,
  asrModels,
  asrActions,
  saveState,
  disabled,
}: {
  settings: Settings;
  profiles: ApiProfileView[];
  applySettings: ApplySettings;
  onRefreshSettings: () => Promise<void>;
  activeTab: ApiSettingsTab;
  onTabChange: (tab: ApiSettingsTab) => void;
  asrModels: RecognitionModels;
  asrActions: RecognitionActions;
  saveState: SaveState;
  disabled: boolean;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  return <div className="settings-section settings-section-active api-settings-view" id="settings-panel-api"
    role="tabpanel" aria-labelledby="settings-tab-api">
    <div className="section-heading">
      <div><KeyRound size={18} /><h2>{t("settings.modelAssignments.title")}</h2></div>
      <div className="theme-toggle-group api-settings-tabs" role="group" aria-label={t("settings.modelAssignments.title")}>
        {(["models", "local", "profiles"] as const).map((value) => <button key={value} type="button"
          aria-pressed={activeTab === value} className={activeTab === value ? "active" : ""}
          onClick={() => onTabChange(value)}>
          {t(value === "local" ? "settings.recognition.localModels" : `settings.modelAssignments.${value}`)}
        </button>)}
      </div>
    </div>
    {activeTab === "models" && <ModelAssignmentsSection settings={settings} profiles={profiles}
      applySettings={applySettings} />}
    {activeTab === "local" && <LocalModelsSection
      settings={settings}
      locale={locale}
      disabled={disabled}
      saveState={saveState}
      models={asrModels}
      actions={asrActions}
    />}
    {activeTab === "profiles" && <ApiManagementSettingsSection
      settings={settings}
      onRefreshSettings={onRefreshSettings}
      embedded
    />}
  </div>;
}
