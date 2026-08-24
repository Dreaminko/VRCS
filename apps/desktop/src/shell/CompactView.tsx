import { useTranslation } from "react-i18next";
import { Maximize2, Mic, Square, X } from "lucide-react";

import type { LookupOrigin } from "../app/app-types";
import { useLivePartial, useTranslationPartials } from "../realtime-state";
import type { Subtitle } from "../subtitles/types";
import { contentLanguageTag } from "../app/ui-language";

export function CompactView({ subtitles, subtitleLimit, selectionActive, running, vrchatMuted, captureDisabled, onSelect, onCapture, onRestore, onClose }: {
  subtitles: Subtitle[];
  subtitleLimit: number;
  selectionActive: boolean;
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
  const partial = selectionActive
    ? undefined
    : microphonePartial ?? speakerPartial;
  const historyLimit = Math.max(0, subtitleLimit - (partial ? 1 : 0));
  const visibleSubtitles = historyLimit > 0
    ? subtitles.slice(-historyLimit)
    : [];
  const latestSubtitle = subtitles.at(-1);
  const captureLabel = t(running ? "capture.pause" : "capture.start");
  return (
    <div className="compact-shell">
      <div className="compact-drag-region" data-tauri-drag-region />
      <div className={`compact-status ${running ? "running" : ""} ${vrchatMuted ? "muted" : ""}`}>
        <i aria-hidden="true" />
        <span>{vrchatMuted ? t("status.pausedVrchatMuted") : partial?.language?.toUpperCase() ?? latestSubtitle?.language?.toUpperCase() ?? "AUTO"}</span>
      </div>
      <div className="compact-content">
        {visibleSubtitles.map((subtitle, index) => (
          <CompactSubtitleRow
            key={subtitle.id ?? subtitle.created_at}
            subtitle={subtitle}
            current={!partial && index === visibleSubtitles.length - 1}
            onSelect={onSelect}
          />
        ))}
        {partial && (
          <div className="compact-subtitle-row compact-subtitle-current">
            <p
              className="compact-original"
              lang={contentLanguageTag(partial.language)}
              onMouseUp={() => void onSelect(partial.text)}
            >
              {partial.text}
            </p>
          </div>
        )}
        {!partial && visibleSubtitles.length === 0 && (
          <div className="compact-subtitle-row compact-subtitle-current">
            <p className="compact-original">{t("live.waiting")}</p>
          </div>
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

function CompactSubtitleRow({ subtitle, current, onSelect }: {
  subtitle: Subtitle;
  current: boolean;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
}) {
  const translationPartial = useTranslationPartials(subtitle.id)[0];
  const visibleTranslation = translationPartial
    ?? subtitle.translation_partial
    ?? subtitle.translations[0];
  const origin: LookupOrigin = {
    id: subtitle.id,
    language: subtitle.language,
    source: subtitle.source ?? null,
    createdAt: subtitle.created_at,
    translation: visibleTranslation?.text ?? null,
  };

  return (
    <div className={`compact-subtitle-row ${current ? "compact-subtitle-current" : "compact-subtitle-history"}`}>
      <p
        className="compact-original"
        lang={contentLanguageTag(subtitle.language)}
        onMouseUp={() => void onSelect(subtitle.text, origin)}
      >
        {subtitle.text}
      </p>
      {visibleTranslation && (
        <p className="compact-translation" lang={contentLanguageTag(visibleTranslation.target_language)}>
          {visibleTranslation.text}
          {(translationPartial || subtitle.translation_partial) && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
        </p>
      )}
    </div>
  );
}
