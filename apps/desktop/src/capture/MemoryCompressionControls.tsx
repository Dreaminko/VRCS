import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { MemoryStick, RefreshCw } from "lucide-react";
import { SettingsDialog } from "../settings/components/SettingsDialog";
import type { PlayerAudioStart } from "./usePlayerAudioStart";

type Memory = PlayerAudioStart["memory"];

function statusKey(memory: Memory) {
  if (memory.status?.supported === false) return "desktopOnly";
  if (memory.status?.enabled === true) return "enabled";
  if (memory.status?.enabled === false) return "disabled";
  if (memory.busy) return "checking";
  return "unknown";
}

export function MemoryCompressionControls({ memory }: { memory: Memory }) {
  const { t } = useTranslation();
  return <div className="memory-compression-setting">
    <div className="memory-compression-heading"><MemoryStick size={18} /><div>
      <strong>{t("settings.audio.memory.title")}</strong>
      <small>{t(`settings.audio.memory.${statusKey(memory)}`)}</small>
    </div></div>
    <button type="button" className="secondary-button" disabled={memory.busy} onClick={memory.open}>{t("settings.audio.memory.manage")}</button>
    {memory.status?.restart_required && <p className="settings-field-note">{t("settings.audio.memory.restart")}</p>}
    {memory.error && <p className="settings-validation-error" role="alert">{memory.error}</p>}
  </div>;
}

export function MemoryCompressionDialog({ memory }: { memory: Memory }) {
  const { t } = useTranslation();
  const focus = useRef<HTMLButtonElement | null>(document.activeElement instanceof HTMLButtonElement ? document.activeElement : null);
  const confirming = memory.dialog === "enable" || memory.dialog === "disable";
  const enable = memory.dialog === "enable";
  const title = t(confirming ? `settings.audio.memory.${enable ? "enableTitle" : "disableTitle"}` : "settings.audio.memory.title");
  return <SettingsDialog label={title} saving={memory.changing} returnFocusRef={focus} autoFocus
    className="player-audio-start-dialog memory-compression-dialog" onClose={memory.close}>
    <div className="api-profile-editor">
      <div className="api-profile-editor-heading"><MemoryStick size={20} /><strong>{title}</strong></div>
      <div className="api-profile-editor-content player-audio-start-copy">
        {confirming ? <>
          <p>{t(`settings.audio.memory.${enable ? "enableImpact" : "disableImpact"}`)}</p>
          <p>{t("settings.audio.memory.confirmRestart")}</p>
        </> : <>
          <p role="status">{t(`settings.audio.memory.${statusKey(memory)}`)}</p>
          {memory.status?.restart_required && <p className="memory-compression-restart" role="status">{t("settings.audio.memory.restart")}</p>}
          {memory.status?.supported && !memory.status.elevated && <p>{t("settings.audio.memory.authorization")}</p>}
        </>}
        {memory.error && <p className="settings-validation-error" role="alert">{memory.error}</p>}
      </div>
      <div className="api-profile-editor-actions">
        {confirming ? <div className="settings-inline-actions">
          <button type="button" className="secondary-button" disabled={memory.changing} onClick={memory.cancelChange}>{t("common.cancel")}</button>
          <button type="button" className="primary-button" disabled={memory.busy} onClick={memory.confirmChange}>{t(`settings.audio.memory.${enable ? "confirmEnable" : "confirmDisable"}`)}</button>
        </div> : <>
          <button type="button" className="secondary-button" disabled={memory.busy} onClick={memory.refresh}><RefreshCw size={15} />{t("settings.audio.memory.refresh")}</button>
          <div className="settings-inline-actions">
            <button type="button" className="secondary-button" disabled={memory.changing} onClick={memory.close}>{t("common.close")}</button>
            {memory.status?.supported && !memory.status.elevated
              ? <button type="button" className="primary-button" disabled={memory.busy} onClick={memory.authorize}>{t("settings.audio.memory.authorize")}</button>
              : memory.status?.enabled !== null && memory.status?.enabled !== undefined && <button type="button" className="primary-button" disabled={memory.busy}
                onClick={() => memory.requestChange(!memory.status!.enabled)}>{t(`settings.audio.memory.${memory.status.enabled ? "disable" : "enable"}`)}</button>}
          </div>
        </>}
      </div>
    </div>
  </SettingsDialog>;
}
