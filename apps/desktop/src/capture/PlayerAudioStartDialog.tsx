import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FlaskConical } from "lucide-react";
import { prefersReducedMotion } from "../app/app-motion";
import { usePlatformCapabilities } from "../core-client/capabilities";
import { SettingsDialog } from "../settings/components/SettingsDialog";
import { playerAudioDisclaimer } from "./player-audio-disclaimer";
import { PLAYER_AUDIO_READING_MS } from "./usePlayerAudioStart";

const DIALOG_CLOSE_MS = 160;

export function PlayerAudioStartDialog({
  openedAt = 0,
  os,
  readOnly = false,
  onDecision,
}: {
  openedAt?: number;
  os?: string;
  readOnly?: boolean;
  onDecision: (accepted: boolean) => void;
}) {
  const { t } = useTranslation();
  const capabilities = usePlatformCapabilities();
  const disclaimer = playerAudioDisclaimer(t, os ?? capabilities.desktop?.os ?? capabilities.core?.os);
  const [remaining, setRemaining] = useState(readOnly ? 0 : 3);
  const [closing, setClosing] = useState(false);
  const focus = useRef<HTMLButtonElement | null>(document.activeElement instanceof HTMLButtonElement ? document.activeElement : null);
  const content = useRef<HTMLDivElement>(null);
  const closeTimerRef = useRef<number | null>(null);
  useEffect(() => () => {
    if (closeTimerRef.current !== null) window.clearTimeout(closeTimerRef.current);
  }, []);
  useEffect(() => {
    content.current?.focus();
    if (readOnly) {
      setRemaining(0);
      return;
    }
    const update = () => setRemaining(Math.max(0, Math.ceil((PLAYER_AUDIO_READING_MS - (performance.now() - openedAt)) / 1_000)));
    update();
    const timer = window.setInterval(update, 100);
    return () => window.clearInterval(timer);
  }, [openedAt, readOnly]);
  const finish = (accepted: boolean) => {
    if (closing || (!readOnly && remaining > 0)) return;
    if (prefersReducedMotion()) {
      onDecision(accepted);
      return;
    }
    setClosing(true);
    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null;
      onDecision(accepted);
    }, DIALOG_CLOSE_MS);
  };
  return <SettingsDialog label={disclaimer.title} saving={!readOnly && remaining > 0} closing={closing} returnFocusRef={focus}
    className="player-audio-start-dialog" onClose={() => finish(false)}>
    <div className="api-profile-editor">
      <div className="api-profile-editor-heading"><FlaskConical size={20} /><strong>{disclaimer.title}</strong></div>
      <div className="api-profile-editor-content player-audio-start-copy" tabIndex={0} ref={content}>
        <p>{disclaimer.intro}</p>
        {disclaimer.sections.map((section) => <section key={section.key}>
          <h3>{section.title}</h3>
          {section.paragraphs.map((paragraph, index) => <p key={index}>{paragraph}</p>)}
        </section>)}
      </div>
      <div className="api-profile-editor-actions">
        {!readOnly && <small role="status" aria-live="polite">{disclaimer.readingStatus(remaining)}</small>}
        <div className="settings-inline-actions">
          {readOnly ? (
            <button type="button" className="primary-button" disabled={closing} onClick={() => finish(false)}>{t("common.close")}</button>
          ) : (
            <>
              <button type="button" className="secondary-button" disabled={remaining > 0 || closing} onClick={() => finish(false)}>{t("common.cancel")}</button>
              <button type="button" className="primary-button" disabled={remaining > 0 || closing} onClick={() => finish(true)}>{disclaimer.confirm}</button>
            </>
          )}
        </div>
      </div>
    </div>
  </SettingsDialog>;
}
