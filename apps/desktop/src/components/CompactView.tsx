import { useTranslation } from "react-i18next";
import { Maximize2, Mic, Square, X } from "lucide-react";

import { useTranslationPartial } from "../realtime-state";
import type { LiveTranscription, Subtitle } from "../types";
import { contentLanguageTag } from "../ui-language";

export function CompactView({ subtitle, partial, running, vrchatMuted, captureDisabled, onSelect, onCapture, onRestore, onClose }: {
  subtitle?: Subtitle;
  partial?: LiveTranscription;
  running: boolean;
  vrchatMuted: boolean;
  captureDisabled: boolean;
  onSelect: (context: string) => Promise<void>;
  onCapture: () => void;
  onRestore: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const translationPartial = useTranslationPartial(subtitle?.id ?? null);
  const visibleTranslation = translationPartial
    ?? subtitle?.translation_partial
    ?? subtitle?.translations.at(-1);
  const captureLabel = t(running ? "capture.pause" : "capture.start");
  return (
    <div className="compact-shell">
      <div className="compact-drag-region" data-tauri-drag-region />
      <div className={`compact-status ${running ? "running" : ""} ${vrchatMuted ? "muted" : ""}`}>
        <i aria-hidden="true" />
        <span>{vrchatMuted ? t("status.pausedVrchatMuted") : partial?.language?.toUpperCase() ?? subtitle?.language?.toUpperCase() ?? "AUTO"}</span>
      </div>
      <div className="compact-content">
        <p className="compact-original" lang={contentLanguageTag(partial?.language ?? subtitle?.language)} onMouseUp={() => (partial?.text || subtitle?.text) && void onSelect(partial?.text ?? subtitle!.text)}>
          {partial?.text ?? subtitle?.text ?? t("live.waiting")}
        </p>
        {!partial && visibleTranslation && (
          <p className="compact-translation" lang={contentLanguageTag(visibleTranslation.target_language)}>
            {visibleTranslation.text}
            {(translationPartial || subtitle?.translation_partial) && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
          </p>
        )}
      </div>
      <div className="compact-actions">
        <button className="compact-capture-button" type="button" aria-label={captureLabel} aria-pressed={running} title={captureLabel} disabled={captureDisabled} onClick={onCapture}>
          {running ? <Square size={15} /> : <Mic size={16} />}
        </button>
        <button className="compact-secondary-action" type="button" aria-label={t("window.restore")} title={t("window.restore")} onClick={onRestore}><Maximize2 size={17} /></button>
        <button className="compact-secondary-action compact-close-button" type="button" aria-label={t("window.close")} title={t("window.close")} onClick={onClose}><X size={17} /></button>
      </div>
    </div>
  );
}
