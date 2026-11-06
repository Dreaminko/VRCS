import { useState } from "react";
import { KeyRound } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ApiProfileView } from "../../providers/types";
import type { ApplySettings } from "../settings-types";
import type { Settings } from "../types";
import { ApiManagementSettingsSection } from "./ApiManagementSettingsSection";
import { ModelAssignmentsSection } from "./ModelAssignmentsSection";

export function ApiSettingsSection({ settings, profiles, applySettings, onRefreshSettings }: {
  settings: Settings;
  profiles: ApiProfileView[];
  applySettings: ApplySettings;
  onRefreshSettings: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState("models");
  return <div className="settings-section settings-section-active api-settings-view" id="settings-panel-api"
    role="tabpanel" aria-labelledby="settings-tab-api">
    <div className="section-heading">
      <div><KeyRound size={18} /><h2>{t("settings.modelAssignments.title")}</h2></div>
      <div className="theme-toggle-group api-settings-tabs" role="group" aria-label={t("settings.modelAssignments.title")}>
        {(["models", "profiles"] as const).map((value) => <button key={value} type="button" aria-pressed={tab === value}
          className={tab === value ? "active" : ""} onClick={() => setTab(value)}>
          {t(`settings.modelAssignments.${value}`)}
        </button>)}
      </div>
    </div>
    {tab === "models" ? <ModelAssignmentsSection settings={settings} profiles={profiles}
      applySettings={applySettings} />
      : <ApiManagementSettingsSection settings={settings} onRefreshSettings={onRefreshSettings} embedded />}
  </div>;
}
