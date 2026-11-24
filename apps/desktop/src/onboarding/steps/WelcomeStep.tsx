import { useId } from "react";
import { AudioLines, Languages, ShieldCheck, UserRound } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { UiLanguagePreference } from "../../app/ui-language";
import { localeCatalog } from "../../i18n/catalog";
import { Select } from "../../settings/SettingsControls";
import { DownloadSourceSettings } from "../../settings/system/DownloadSourceSettings";
import { ThemeSettings } from "../../settings/system/ThemeSettings";
import type { FeatureSettings, ModelDownloadSource } from "../../settings/types";
import { FeatureModeSection } from "./FeatureModeSection";

export function WelcomeStep({
  languagePreference,
  busy,
  onUpdateLanguage,
  playerId,
  onChangePlayerId,
  onCommitPlayerId,
  features,
  onUpdateFeatures,
  downloadSource,
  onUpdateDownloadSource,
}: {
  languagePreference: UiLanguagePreference;
  busy: boolean;
  onUpdateLanguage: (preference: UiLanguagePreference) => void;
  playerId?: string;
  onChangePlayerId?: (value: string) => void;
  onCommitPlayerId?: () => void;
  features?: FeatureSettings;
  onUpdateFeatures?: (update: (current: FeatureSettings) => FeatureSettings) => Promise<boolean>;
  downloadSource: ModelDownloadSource;
  onUpdateDownloadSource: (source: ModelDownloadSource) => void;
}) {
  const { t } = useTranslation();
  const playerIdInputId = useId();

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
      <ThemeSettings variant="onboarding" disabled={busy} />
      {onChangePlayerId && (
        <div className="onboarding-language-setting">
          <div className="onboarding-language-copy">
            <UserRound size={18} />
            <span>
              <label htmlFor={playerIdInputId}><strong>{t("settings.system.vrchatPlayerId")}</strong></label>
              <small>{t("onboarding.welcome.playerIdDescription")}</small>
            </span>
          </div>
          <input
            id={playerIdInputId}
            type="text"
            maxLength={128}
            value={playerId ?? ""}
            placeholder={t("settings.system.vrchatPlayerIdPlaceholder")}
            disabled={busy}
            onChange={(event) => onChangePlayerId(event.target.value)}
            onBlur={() => onCommitPlayerId?.()}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                event.currentTarget.blur();
              }
            }}
          />
        </div>
      )}
      <DownloadSourceSettings
        source={downloadSource}
        disabled={busy}
        variant="onboarding"
        onChange={onUpdateDownloadSource}
      />
      {features && onUpdateFeatures && (
        <FeatureModeSection
          features={features}
          disabled={busy}
          onUpdateFeatures={onUpdateFeatures}
        />
      )}
    </div>
  );
}
