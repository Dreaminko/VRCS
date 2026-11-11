import { MessageSquare } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ChatboxStatus } from "../../core-client/types";
import type { ApiProfileView } from "../../providers/types";
import type { ApplySettings, SaveState } from "../settings-types";
import type { Settings, TranslationSettings } from "../types";
import { PreferenceToggle } from "../SettingsControls";
import { chatboxTranslationReady, DEFAULT_VRCHAT_CHATBOX_CONFIG } from "../vrchat-chatbox-settings";

export function VrchatChatboxSettings({ draft, translation, profiles, status, supported, saveState, saveMessage, applySettings, onOpenTranslation }: {
  draft: Settings;
  translation: TranslationSettings;
  profiles: ApiProfileView[];
  status?: ChatboxStatus | null;
  supported?: boolean;
  saveState: SaveState;
  saveMessage?: string;
  applySettings: ApplySettings;
  onOpenTranslation: () => void;
}) {
  const { t } = useTranslation();
  const networkEnabled = draft.audio.output.player_audio?.enabled === true;
  const enabled = (draft.vrchat_chatbox ?? DEFAULT_VRCHAT_CHATBOX_CONFIG).enabled;
  const phase = status?.phase ?? "stopped";
  const showTranslationHint = !chatboxTranslationReady(translation, profiles);
  const showIssue = supported === false || phase === "failed";
  const saveError = saveState === "error" ? saveMessage : undefined;
  return <section className="capture-chatbox-settings" aria-labelledby="vrchat-chatbox-title">
    <div className="device-group-heading">
      <div className="device-group-title">
        <span className="device-group-icon"><MessageSquare size={18} /></span>
        <div>
          <h3 id="vrchat-chatbox-title">{t("settings.capture.chatbox.title")}</h3>
        </div>
      </div>
      <PreferenceToggle title={t("settings.capture.chatbox.enable")} hideLabel
        tooltip={!enabled && !networkEnabled ? t("settings.capture.networkRequired") : undefined}
        checked={enabled}
        disabled={!enabled && (supported === false || !networkEnabled)}
        onChange={(enabled) => applySettings((current) => ({ ...current, vrchat_chatbox: { enabled } }))} />
    </div>
    {(showTranslationHint || showIssue || saveError) && <div className="device-group-content">
      {showTranslationHint && <div className="capture-chatbox-translation">
        <p className="settings-field-note">{t("settings.capture.chatbox.translationUnavailable")}</p>
        <button type="button" className="secondary-button" onClick={onOpenTranslation}>{t("settings.capture.openTranslation")}</button>
      </div>}
      {showIssue && <div className="settings-field-note" role="status" aria-live="polite">
        <span>{supported === false ? t("settings.capture.unsupported") : t("settings.capture.chatbox.phase.failed")}</span>
        {status?.detail && <p className="settings-validation-error">{status.detail}</p>}
      </div>}
      {saveError && <p className="settings-validation-error" role="alert">{saveError}</p>}
    </div>}
  </section>;
}
