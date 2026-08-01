import { useTranslation } from "react-i18next";
import { MessageSquare, Mic, Square, X } from "lucide-react";

import type { LiveTranscription, Subtitle } from "../types";

export function CompactView({ subtitle, partial, running, captureDisabled, onSelect, onCapture, onRestore, onClose }: {
  subtitle?: Subtitle;
  partial?: LiveTranscription;
  running: boolean;
  captureDisabled: boolean;
  onSelect: (context: string) => Promise<void>;
  onCapture: () => void;
  onRestore: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const captureLabel = t(running ? "capture.pause" : "capture.start");
  return (
    <div className="compact-shell">
      <div className="compact-drag-region" data-tauri-drag-region />
      <div className="compact-status"><i className={running ? "running" : ""} />{partial?.language?.toUpperCase() ?? subtitle?.language?.toUpperCase() ?? "AUTO"}</div>
      <p onMouseUp={() => (partial?.text || subtitle?.text) && void onSelect(partial?.text ?? subtitle!.text)}>{partial?.text ?? subtitle?.text ?? t("live.waiting")}</p>
      <div className="compact-actions">
        <button className={`compact-capture-button ${running ? "running" : ""}`} type="button" aria-label={captureLabel} title={captureLabel} disabled={captureDisabled} onClick={onCapture}>
          {running ? <Square size={15} /> : <Mic size={16} />}
        </button>
        <button className="compact-secondary-action" type="button" aria-label={t("window.restore")} title={t("window.restore")} onClick={onRestore}><MessageSquare size={17} /></button>
        <button className="compact-secondary-action" type="button" aria-label={t("window.close")} title={t("window.close")} onClick={onClose}><X size={17} /></button>
      </div>
    </div>
  );
}
