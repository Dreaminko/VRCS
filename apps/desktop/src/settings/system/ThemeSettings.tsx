import { SunMoon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useAppTheme } from "../../app/useAppTheme";

/** Theme preference selector shared between system settings and onboarding. */
export function ThemeSettings({
  disabled = false,
  variant = "system",
}: {
  disabled?: boolean;
  variant?: "system" | "onboarding";
}) {
  const { t } = useTranslation();
  const { themePreference, setThemePreference } = useAppTheme();

  const toggleGroup = (
    <div className="theme-toggle-group" role="group" aria-label={t("settings.system.theme")}>
      {([
        ["system", t("settings.system.followSystem")],
        ["light", t("settings.system.themeLight")],
        ["dark", t("settings.system.themeDark")],
      ] as const).map(([value, label]) => (
        <button
          key={value}
          type="button"
          aria-pressed={themePreference === value}
          className={themePreference === value ? "active" : ""}
          disabled={disabled}
          onClick={() => setThemePreference(value)}
        >
          {label}
        </button>
      ))}
    </div>
  );

  if (variant === "onboarding") {
    return (
      <div className="onboarding-language-setting onboarding-theme-setting">
        <div className="onboarding-language-copy">
          <SunMoon size={18} />
          <span>
            <strong>{t("settings.system.theme")}</strong>
            <small>{t("settings.system.themeDescription")}</small>
          </span>
        </div>
        {toggleGroup}
      </div>
    );
  }

  return (
    <div className="system-theme-setting">
      <div>
        <strong>{t("settings.system.theme")}</strong>
      </div>
      {toggleGroup}
    </div>
  );
}
