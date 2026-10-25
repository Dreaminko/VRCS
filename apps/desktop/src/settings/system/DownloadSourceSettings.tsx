import { useTranslation } from "react-i18next";

import { Select } from "../SettingsControls";
import type { ModelDownloadSource } from "../types";

export function DownloadSourceSettings({ source, disabled = false, onChange }: {
  source: ModelDownloadSource;
  disabled?: boolean;
  onChange: (source: ModelDownloadSource) => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="system-select-setting download-source-settings">
      <div><strong>{t("settings.downloads.source")}</strong></div>
      <Select label={t("settings.downloads.source")} value={source}
        options={(["auto", "official", "modelscope"] as const).map((value) => ({
          value, label: t(`settings.downloads.${value}`),
        }))}
        disabled={disabled} hideLabel onChange={(value) => onChange(value as ModelDownloadSource)} />
    </div>
  );
}
