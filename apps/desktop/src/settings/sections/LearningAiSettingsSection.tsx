import { Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { DropdownField, EditableDropdownField } from "../../shared/ui/DropdownField";
import { useLearningAiConfiguration } from "../../learning/hooks/useLearningAiConfiguration";
import { localeCatalog } from "../../i18n/catalog";

export function LearningAiSettingsSection() {
  const { t } = useTranslation();
  const ai = useLearningAiConfiguration(true);
  const profileOptions = ai.profiles.length
    ? ai.profiles.map((profile) => ({
        value: profile.id,
        label: profile.name || profile.provider_display_name,
        description: profile.provider_display_name,
      }))
    : [{
        value: "",
        label: ai.profilesLoading ? t("common.loading") : t("settings.learning.noAiProfiles"),
      }];

  return (
    <section className="learning-settings-group learning-ai-settings" aria-labelledby="learning-ai-settings-title">
      <div className="section-heading">
        <div>
          <Sparkles size={18} />
          <h3 id="learning-ai-settings-title">{t("learning.ai.title")}</h3>
        </div>
      </div>
      <p className="learning-ai-settings-description">{t("settings.learning.aiDescription")}</p>

      {ai.error && <p className="learning-ai-settings-message error" role="alert">{ai.error}</p>}

      <div className="learning-ai-settings-grid">
        <div className="field">
          <span>{t("learning.ai.profile")}</span>
          <DropdownField
            label={t("learning.ai.profile")}
            value={ai.preferences.profileId}
            options={profileOptions}
            disabled={ai.profilesLoading || !ai.profiles.length}
            onChange={ai.setProfileId}
          />
        </div>
        <div className="field">
          <span>{t("learning.ai.model")}</span>
          <EditableDropdownField
            label={t("learning.ai.model")}
            value={ai.preferences.model}
            options={ai.models.map((model) => ({ value: model, label: model }))}
            disabled={!ai.preferences.profileId}
            optionsDisabled={ai.modelsLoading || !ai.models.length}
            placeholder={t("learning.ai.modelPlaceholder")}
            onChange={ai.setModel}
          />
          {ai.modelsError && <small className="learning-ai-settings-message error">{ai.modelsError}</small>}
        </div>
        <div className="field">
          <span>{t("learning.ai.explanationLanguage")}</span>
          <DropdownField
            label={t("learning.ai.explanationLanguage")}
            value={ai.explanationLanguagePreference}
            options={explanationLanguageOptions(t)}
            onChange={ai.setExplanationLanguage}
          />
        </div>
        <div className="field">
          <span>{t("learning.ai.explanationLevel")}</span>
          <DropdownField
            label={t("learning.ai.explanationLevel")}
            value={ai.preferences.explanationLevel}
            options={explanationLevelOptions(t)}
            onChange={ai.setExplanationLevel}
          />
        </div>
      </div>

      <div className={`learning-ai-privacy-note ${ai.selectedProfile?.capabilities.is_local ? "local" : "cloud"}`}>
        <strong>{ai.selectedProfile?.capabilities.is_local ? t("learning.ai.localNoticeTitle") : t("learning.ai.cloudNoticeTitle")}</strong>
        <p>{ai.selectedProfile
          ? t(ai.selectedProfile.capabilities.is_local ? "learning.ai.localNotice" : "learning.ai.cloudNotice", {
              provider: ai.selectedProfile.provider_display_name,
              model: ai.preferences.model || t("learning.ai.modelUnset"),
            })
          : t("learning.ai.profileMissing")}</p>
      </div>
    </section>
  );
}

function explanationLanguageOptions(t: (key: string) => string) {
  return [
    { value: "ui", label: t("learning.ai.followUiLanguage") },
    ...localeCatalog.map(({ _meta }) => ({ value: _meta.locale, label: _meta.name })),
  ];
}

function explanationLevelOptions(t: (key: string) => string) {
  return [
    { value: "beginner", label: t("learning.ai.levels.beginner") },
    { value: "intermediate", label: t("learning.ai.levels.intermediate") },
    { value: "advanced", label: t("learning.ai.levels.advanced") },
  ];
}
