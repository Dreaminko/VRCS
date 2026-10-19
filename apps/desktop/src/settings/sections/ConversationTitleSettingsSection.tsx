import { useState } from "react";
import { ChevronDown, MessageSquareText, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { localeCatalog } from "../../i18n/catalog";
import type { ApiProfileView } from "../../providers/types";
import { readTranscriptionStartBehavior, writeTranscriptionStartBehavior, type TranscriptionStartBehavior } from "../../capture/transcription-start";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { PreferenceToggle, Select } from "../SettingsControls";
import { useTranslationProfileModels } from "../hooks/useTranslationProfileModels";
import type { ConversationTitleSettings } from "../types";

// Mirrors TITLE_OUTPUT_LANGUAGES in core; new UI locales must be added there first.
const TITLE_OUTPUT_LANGUAGES = new Set(["en-US", "ja-JP", "zh-CN", "zh-Hant"]);

export function ConversationTitleSettingsSection({ config, profiles, onChange, onManageProfiles }: {
  config: ConversationTitleSettings;
  profiles: ApiProfileView[];
  onChange: (patch: Partial<ConversationTitleSettings>) => void;
  onManageProfiles: () => void;
}) {
  const { t } = useTranslation();
  const [startBehavior, setStartBehavior] = useState(readTranscriptionStartBehavior);
  const compatible = profiles.filter((profile) => profile.capabilities.supports_text_generation);
  const selected = compatible.find((profile) => profile.id === config.profile_id);
  const catalog = useTranslationProfileModels(selected, Boolean(selected));
  const configured = Boolean(selected && config.model.trim());
  const [modelSettingsOpen, setModelSettingsOpen] = useState(!configured || config.auto_generate);

  return <section className="system-settings-group" aria-labelledby="system-conversations-title">
    <div className="section-heading"><div><MessageSquareText size={18} />
      <h3 id="system-conversations-title">{t("settings.conversationTitles.title")}</h3>
    </div></div>
    <div className="system-select-setting">
      <div><strong>{t("settings.system.transcriptionStartBehavior")}</strong></div>
      <Select label={t("settings.system.transcriptionStartBehavior")} value={startBehavior} hideLabel disabled={false}
        options={[
          { value: "continue_current", label: t("settings.system.continueCurrentConversation") },
          { value: "new_conversation", label: t("settings.system.createNewConversation") },
        ]}
        onChange={(value) => {
          const behavior = value as TranscriptionStartBehavior;
          setStartBehavior(behavior);
          writeTranscriptionStartBehavior(behavior);
        }} />
    </div>
    <PreferenceToggle title={t("settings.conversationTitles.autoGenerate")}
      description={t("settings.conversationTitles.description")}
      checked={config.auto_generate} disabled={!configured && !config.auto_generate}
      onChange={(auto_generate) => onChange({ auto_generate })} />
    {!configured && <p className="settings-field-note">{t("settings.conversationTitles.configureFirst")}</p>}
    <details className="vision-settings-details" open={modelSettingsOpen}
      onToggle={(event) => setModelSettingsOpen(event.currentTarget.open)}>
      <summary><span>{t("settings.conversationTitles.modelSettings")}</span><ChevronDown size={16} /></summary>
      <div className="vr-overlay-field-grid">
        <Select label={t("settings.conversationTitles.profile")} value={config.profile_id ?? ""} disabled={false}
          helper={!compatible.length ? t("settings.conversationTitles.noProfiles") : undefined}
          options={[
            { value: "", label: t("settings.conversationTitles.selectProfile") },
            ...compatible.map((profile) => ({ value: profile.id, label: profile.name })),
            ...(config.profile_id && !selected ? [{ value: config.profile_id, label: t("settings.conversationTitles.unavailableProfile") }] : []),
          ]}
          onChange={(profile_id) => onChange({ profile_id: profile_id || null, model: "", auto_generate: false })} />
        <Select label={t("settings.conversationTitles.language")} value={config.output_language} disabled={false}
          options={[
            { value: "auto", label: t("settings.conversationTitles.autoLanguage") },
            ...localeCatalog
              .filter(({ _meta }) => TITLE_OUTPUT_LANGUAGES.has(_meta.locale))
              .map(({ _meta }) => ({ value: _meta.locale, label: _meta.name })),
          ]}
          onChange={(output_language) => onChange({ output_language })} />
        <div className="field">
          <span>{t("settings.conversationTitles.model")}</span>
          <div className="translation-model-input-row">
            <EditableDropdownField label={t("settings.conversationTitles.model")} value={config.model}
              placeholder={t("settings.conversationTitles.selectModel")}
              disabled={!selected} optionsDisabled={catalog.loading}
              options={catalog.models.map((model) => ({ value: model, label: model }))} commitOnBlur
              onChange={(model) => onChange({ model: model.trim(), ...(!model.trim() ? { auto_generate: false } : {}) })} />
            <button className="secondary-button" type="button" aria-label={t("settings.apiManagement.refreshModels")}
              title={t("settings.apiManagement.refreshModels")} disabled={!selected || catalog.loading}
              onClick={() => void catalog.refresh()}>
              <RefreshCw size={15} className={catalog.loading ? "spinning" : undefined} />
            </button>
          </div>
          {catalog.error && <small className="api-model-catalog-error" role="alert">{catalog.error}</small>}
        </div>
      </div>
      <p className="settings-field-note">{t("settings.conversationTitles.privacy")}</p>
      <button className="secondary-button" type="button" onClick={onManageProfiles}>{t("settings.conversationTitles.manageProfiles")}</button>
    </details>
  </section>;
}
