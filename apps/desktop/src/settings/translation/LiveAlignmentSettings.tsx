import { useTranslation } from "react-i18next";
import type { LiveAlignmentSettings as Settings } from "../types";
import { PreferenceToggle } from "../SettingsControls";

export function LiveAlignmentSettings({ value, disabled, onChange }: {
  value: Settings | undefined;
  disabled: boolean;
  onChange: (value: Settings) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="translation-config-row translation-alignment-row">
      <PreferenceToggle title={t("settings.translation.alignment.enabled")}
        description={t("settings.translation.alignment.description")}
        checked={value?.enabled ?? true} disabled={disabled}
        onChange={(enabled) => onChange({ enabled })} />
    </div>
  );
}
