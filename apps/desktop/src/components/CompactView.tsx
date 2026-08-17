import { useTranslation } from "react-i18next";
import { Maximize2, Mic, Square, X } from "lucide-react";

import type { LookupOrigin } from "../app-types";
import { useLivePartial, useTranslationPartial } from "../realtime-state";
import type { Subtitle } from "../types";
import { contentLanguageTag } from "../ui-language";

export function CompactView({ subtitle, running, vrchatMuted, captureDisabled, onSelect, onCapture, onRestore, onClose }: {
  subtitle?: Subtitle;
  running: boolean;
  vrchatMuted: boolean;
  captureDisabled: boolean;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onCapture: () => void;
  onRestore: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const microphonePartial = useLivePartial("microphone");
  const speakerPartial = useLivePartial("speaker");
  const partial = microphonePartial ?? speakerPartial;
  const translationPartial = useTranslationPartial(subtitle?.id ?? null);
  const visibleTranslation = translationPartial
    ?? subtitle?.translation_partial
    ?? subtitle?.translations.at(-1);
  const captureLabel = t(running ? "capture.pause" : "capture.start");
  const origin: LookupOrigin | undefined = subtitle ? {
    id: subtitle.id,
    language: subtitle.language,
    source: subtitle.source ?? null,
    createdAt: subtitle.created_at,
    translation: visibleTranslation?.text ?? null,
  } : undefined;
  return (
    <div className="compact-shell">
      <div className="compact-drag-region" data-tauri-drag-region />
      <div className={`compact-status ${running ? "running" : ""} ${vrchatMuted ? "muted" : ""}`}>
        <i aria-hidden="true" />
        <span>{vrchatMuted ? t("status.pausedVrchatMuted") : partial?.language?.toUpperCase() ?? subtitle?.language?.toUpperCase() ?? "AUTO"}</span>
      </div>
      <div className="compact-content">
        <p className="compact-original" lang={contentLanguageTag(partial?.language ?? subtitle?.language)} onMouseUp={() => (partial?.text || subtitle?.text) && void onSelect(partial?.text ?? subtitle!.text, partial ? undefined : origin)}>
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
