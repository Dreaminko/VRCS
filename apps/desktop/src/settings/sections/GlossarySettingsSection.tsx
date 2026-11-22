import { BookOpenText } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Settings } from "../types";
import { GlossaryEditor } from "../glossary/GlossaryEditor";
import type { ApplySettings } from "../settings-types";
import { PreferenceToggle } from "../SettingsControls";

export function GlossarySettingsSection({
  draft,
  applySettings,
  onClearSettingsError,
}: {
  draft: Settings;
  applySettings: ApplySettings;
  onClearSettingsError?: () => void;
}) {
  const { t } = useTranslation();
  const updateGlossary = (patch: Partial<Settings["glossary"]>) => applySettings((current) => ({
    ...current,
    glossary: { ...current.glossary, ...patch },
  }));

  return (
    <div className="settings-section settings-section-active glossary-section" id="settings-panel-glossary" role="tabpanel" aria-labelledby="settings-tab-glossary">
      <div className="section-heading">
        <div>
          <BookOpenText size={18} />
          <h2>{t("settings.glossary.title")}</h2>
        </div>
      </div>

      <div className="settings-toggle-list glossary-usage-toggles">
        <PreferenceToggle
          title={t("settings.glossary.llmEnabled")}
          description={t("settings.glossary.llmEnabledDescription")}
          checked={draft.glossary.llm_enabled}
          disabled={false}
          onChange={(llm_enabled) => updateGlossary({ llm_enabled })}
        />
        <PreferenceToggle
          title={t("settings.glossary.asrEnabled")}
          description={t("settings.glossary.asrEnabledDescription")}
          checked={draft.glossary.asr_enabled}
          disabled={false}
          onChange={(asr_enabled) => updateGlossary({ asr_enabled })}
        />
      </div>

      <GlossaryEditor
        sources={draft.glossary.sources}
        disabled={false}
        onClearSettingsError={onClearSettingsError}
        onChange={(sources, afterSave, afterError) => applySettings((current) => ({
          ...current,
          glossary: { ...current.glossary, sources },
        }), afterSave, afterError)}
      />
    </div>
  );
}
