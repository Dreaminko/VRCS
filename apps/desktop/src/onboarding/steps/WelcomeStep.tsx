import { AudioLines, Languages, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { UiLanguagePreference } from "../../app/ui-language";
import { localeCatalog } from "../../i18n/catalog";
import { Select } from "../../settings/SettingsControls";

export function WelcomeStep({
  languagePreference,
  busy,
  onUpdateLanguage,
}: {
  languagePreference: UiLanguagePreference;
  busy: boolean;
  onUpdateLanguage: (preference: UiLanguagePreference) => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="onboarding-welcome">
      <h2>{t("onboarding.welcome.title")}</h2>
      <p>{t("onboarding.welcome.description")}</p>
      <div className="onboarding-info-grid">
        <article><ShieldCheck size={19} /><strong>{t("onboarding.welcome.privacyTitle")}</strong><span>{t("onboarding.welcome.privacyDescription")}</span></article>
        <article><AudioLines size={19} /><strong>{t("onboarding.welcome.realtimeTitle")}</strong><span>{t("onboarding.welcome.realtimeDescription")}</span></article>
      </div>
      <div className="onboarding-language-setting">
        <div className="onboarding-language-copy"><Languages size={18} /><span><strong>{t("settings.system.language")}</strong><small>{t("settings.system.languageDescription")}</small></span></div>
        <Select
          label={t("settings.system.language")}
          value={languagePreference}
          options={[
            { value: "system", label: t("settings.system.followSystem") },
            ...localeCatalog.map(({ _meta }) => ({ value: _meta.locale, label: _meta.name })),
          ]}
          disabled={busy}
          hideLabel
          onChange={(value) => onUpdateLanguage(value as UiLanguagePreference)}
        />
      </div>
    </div>
  );
}
