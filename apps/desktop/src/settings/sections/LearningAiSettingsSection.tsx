import { Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { DropdownField } from "../../shared/ui/DropdownField";
import { useLearningAiConfiguration } from "../../learning/hooks/useLearningAiConfiguration";
import { localeCatalog } from "../../i18n/catalog";
import type { Settings } from "../types";

export function LearningAiSettingsSection({ settings }: {
  settings: Settings;
}) {
  const { t } = useTranslation();
  const ai = useLearningAiConfiguration(settings);
  return (
    <section className="learning-settings-group learning-ai-settings" aria-labelledby="learning-ai-settings-title">
      <div className="section-heading">
        <div>
          <Sparkles size={18} />
          <h3 id="learning-ai-settings-title">{t("learning.ai.title")}</h3>
        </div>
      </div>
      <p className="learning-ai-settings-description">{t("settings.learning.aiDescription")}</p>
      <div className="learning-ai-settings-grid">
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
