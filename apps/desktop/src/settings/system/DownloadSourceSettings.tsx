import { Download } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Select } from "../SettingsControls";
import type { ModelDownloadSource } from "../types";

export function DownloadSourceSettings({
  source,
  disabled = false,
  variant = "system",
  onChange,
}: {
  source: ModelDownloadSource;
  disabled?: boolean;
  variant?: "system" | "onboarding";
  onChange: (source: ModelDownloadSource) => void;
}) {
  const { t } = useTranslation();
  const options = (["auto", "official", "modelscope"] as const).map((value) => ({
    value,
    label: t(`settings.downloads.${value}`),
  }));

  if (variant === "onboarding") {
    return (
      <div className="onboarding-language-setting onboarding-download-setting download-source-settings">
        <div className="onboarding-language-copy">
          <Download size={18} />
          <span>
            <strong>{t("settings.downloads.source")}</strong>
            <small>{t("settings.downloads.sourceDescription")}</small>
          </span>
        </div>
        <Select
          label={t("settings.downloads.source")}
          value={source}
          options={options}
          disabled={disabled}
          hideLabel
          onChange={(value) => onChange(value as ModelDownloadSource)}
        />
      </div>
    );
  }

  return (
    <div className="system-select-setting download-source-settings">
      <div><strong>{t("settings.downloads.source")}</strong></div>
      <Select
        label={t("settings.downloads.source")}
        value={source}
        options={options}
        disabled={disabled}
        hideLabel
        onChange={(value) => onChange(value as ModelDownloadSource)}
      />
    </div>
  );
}
