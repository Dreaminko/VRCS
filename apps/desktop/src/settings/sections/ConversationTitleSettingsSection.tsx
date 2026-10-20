import { useState } from "react";
import { MessageSquareText, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { localeCatalog } from "../../i18n/catalog";
import type { ApiProfileView } from "../../providers/types";
import { readTranscriptionStartBehavior, writeTranscriptionStartBehavior, type TranscriptionStartBehavior } from "../../capture/transcription-start";
import { EditableDropdownField } from "../../shared/ui/DropdownField";
import { PreferenceToggle, Select } from "../SettingsControls";
import { useProfileModels } from "../hooks/useProfileModels";
import type { ConversationTitleSettings } from "../types";

// Mirrors TITLE_OUTPUT_LANGUAGES in core; new UI locales must be added there first.
const TITLE_OUTPUT_LANGUAGES = new Set(["en-US", "ja-JP", "zh-CN", "zh-Hant"]);

export function ConversationTitleSettingsSection({ config, profiles, onChange }: {
  config: ConversationTitleSettings;
  profiles: ApiProfileView[];
  onChange: (patch: Partial<ConversationTitleSettings>) => void;
}) {
  const { t } = useTranslation();
  const [startBehavior, setStartBehavior] = useState(readTranscriptionStartBehavior);
  const compatible = profiles.filter((profile) => profile.capabilities.supports_text_generation);
  const selected = compatible.find((profile) => profile.id === config.profile_id);
  const catalog = useProfileModels(selected, Boolean(selected));
  const configured = Boolean(selected && config.model.trim());
  const profileLabel = (profile: ApiProfileView) => profile.name === profile.provider_display_name
    ? profile.name
    : `${profile.name} · ${profile.provider_display_name}`;
  // The saved profile is missing or lacks text generation; keep it listed so the trigger shows what broke.
  const unavailableName = config.profile_id && !selected
    ? profiles.find((profile) => profile.id === config.profile_id)?.name ?? config.profile_id
    : undefined;

  return <section className="system-settings-group" aria-labelledby="system-conversations-title">
    <div className="section-heading"><div><MessageSquareText size={18} />
      <h3 id="system-conversations-title">{t("settings.conversationTitles.title")}</h3>
    </div></div>
    <div className="system-select-setting">
      <div><strong>{t("settings.system.transcriptionStartBehavior")}</strong></div>
      <Select label={t("settings.system.transcriptionStartBehavior")} value={startBehavior} hideLabel
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
    {/* Stay clickable when auto-generate is on but its profile/model became invalid, so it can still be turned off. */}
    <PreferenceToggle title={t("settings.conversationTitles.autoGenerate")}
      description={t("settings.conversationTitles.description")}
      checked={config.auto_generate} disabled={!configured && !config.auto_generate}
      onChange={(auto_generate) => onChange({ auto_generate })} />
    {!configured && <p className="settings-field-note">{t("settings.conversationTitles.configureFirst")}</p>}
    <div className="conversation-title-model-settings">
      <strong>{t("settings.conversationTitles.modelSettings")}</strong>
      <div className="vr-overlay-field-grid">
        <Select label={t("settings.conversationTitles.profile")} value={config.profile_id ?? ""} floating="page"
          placeholder={t("settings.conversationTitles.selectProfile")}
          helper={!compatible.length ? t("settings.conversationTitles.noProfiles") : undefined}
          options={[
            ...compatible.map((profile) => ({ value: profile.id, label: profileLabel(profile) })),
            ...(unavailableName ? [{ value: config.profile_id ?? "", label: `${unavailableName} · ${t("settings.conversationTitles.unavailableProfile")}` }] : []),
          ]}
          // Model catalogs differ per profile, so switching clears the model; auto-generate requires one.
          onChange={(profile_id) => onChange({ profile_id, model: "", auto_generate: false })} />
        <Select label={t("settings.conversationTitles.language")} value={config.output_language} floating="page"
          options={[
            { value: "auto", label: t("settings.conversationTitles.autoLanguage") },
            ...localeCatalog
              .filter(({ _meta }) => TITLE_OUTPUT_LANGUAGES.has(_meta.locale))
              .map(({ _meta }) => ({ value: _meta.locale, label: _meta.name })),
          ]}
          onChange={(output_language) => onChange({ output_language })} />
        <div className="field conversation-title-model-field">
          <span>{t("settings.conversationTitles.model")}</span>
          <div className="translation-model-input-row">
            <EditableDropdownField label={t("settings.conversationTitles.model")} value={config.model} floating
              placeholder={t("settings.conversationTitles.selectModel")}
              disabled={!selected} optionsDisabled={catalog.loading}
              options={catalog.models.map((model) => ({ value: model, label: model }))} commitOnBlur
              onChange={(model) => {
                const trimmed = model.trim();
                // An empty model invalidates auto-generation.
                onChange(trimmed ? { model: trimmed } : { model: "", auto_generate: false });
              }} />
            <button className="secondary-button" type="button" aria-label={t("settings.apiManagement.refreshModels")}
              title={t("settings.apiManagement.refreshModels")} disabled={!selected || catalog.loading}
              onClick={() => void catalog.refresh()}>
              <RefreshCw size={15} className={catalog.loading ? "spinning" : undefined} />
            </button>
          </div>
          {catalog.error && <small className="api-model-catalog-error" role="alert">{catalog.error}</small>}
        </div>
      </div>
    </div>
  </section>;
}
