import { useState } from "react";
import { MessageSquareText } from "lucide-react";
import { useTranslation } from "react-i18next";

import { localeCatalog } from "../../i18n/catalog";
import type { ApiProfileView } from "../../providers/types";
import { readTranscriptionStartBehavior, writeTranscriptionStartBehavior, type TranscriptionStartBehavior } from "../../capture/transcription-start";
import { PreferenceToggle, Select } from "../SettingsControls";
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
  const configured = Boolean(selected && config.model.trim());

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
      checked={config.auto_generate} disabled={!configured && !config.auto_generate}
      onChange={(auto_generate) => onChange({ auto_generate })} />
    {!configured && <p className="settings-field-note">{t("settings.conversationTitles.configureFirst")}</p>}
    <Select label={t("settings.conversationTitles.language")} value={config.output_language} floating="page"
      options={[
        { value: "auto", label: t("settings.conversationTitles.autoLanguage") },
        ...localeCatalog.filter(({ _meta }) => TITLE_OUTPUT_LANGUAGES.has(_meta.locale))
          .map(({ _meta }) => ({ value: _meta.locale, label: _meta.name })),
      ]} onChange={(output_language) => onChange({ output_language })} />
  </section>;
}
