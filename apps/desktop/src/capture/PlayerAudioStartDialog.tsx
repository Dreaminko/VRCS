import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FlaskConical } from "lucide-react";
import { SettingsDialog } from "../settings/components/SettingsDialog";
import { PLAYER_AUDIO_READING_MS } from "./usePlayerAudioStart";

export function PlayerAudioStartDialog({ openedAt, onDecision }: { openedAt: number; onDecision: (accepted: boolean) => void }) {
  const { t } = useTranslation();
  const [remaining, setRemaining] = useState(3);
  const focus = useRef<HTMLButtonElement | null>(document.activeElement instanceof HTMLButtonElement ? document.activeElement : null);
  const content = useRef<HTMLDivElement>(null);
  useEffect(() => {
    content.current?.focus();
    const update = () => setRemaining(Math.max(0, Math.ceil((PLAYER_AUDIO_READING_MS - (performance.now() - openedAt)) / 1_000)));
    update();
    const timer = window.setInterval(update, 100);
    return () => window.clearInterval(timer);
  }, [openedAt]);
  return <SettingsDialog label={t("warnings.playerAudio.title")} saving={remaining > 0} returnFocusRef={focus}
    className="player-audio-start-dialog" onClose={() => onDecision(false)}>
    <div className="api-profile-editor">
      <div className="api-profile-editor-heading"><FlaskConical size={20} /><strong>{t("warnings.playerAudio.title")}</strong></div>
      <div className="api-profile-editor-content player-audio-start-copy" tabIndex={0} ref={content}>
        <p>{t("warnings.playerAudio.intro")}</p>
        {["principle", "terms", "anticheat", "privacy", "disclaimer"].map((key) => <section key={key}>
          <h3>{t(`warnings.playerAudio.${key}Title`)}</h3>
          <p>{t(`warnings.playerAudio.${key}`)}</p>
          {key === "principle" && <>
            <p>{t("warnings.playerAudio.paging")}</p>
            <p>{t("settings.audio.players.nameHint")}</p>
          </>}
          {key === "privacy" && <p>{t("warnings.playerAudio.privacyConsent")}</p>}
        </section>)}
      </div>
      <div className="api-profile-editor-actions">
        <small role="status" aria-live="polite">{remaining > 0 ? t("warnings.playerAudio.countdown", { seconds: remaining }) : t("warnings.playerAudio.ready")}</small>
        <div className="settings-inline-actions">
          <button type="button" className="secondary-button" disabled={remaining > 0} onClick={() => onDecision(false)}>{t("common.cancel")}</button>
          <button type="button" className="primary-button" disabled={remaining > 0} onClick={() => onDecision(true)}>{t("warnings.playerAudio.confirm")}</button>
        </div>
      </div>
    </div>
  </SettingsDialog>;
}
