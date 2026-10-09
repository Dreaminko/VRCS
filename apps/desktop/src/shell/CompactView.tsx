import { useTranslation } from "react-i18next";
import { useLayoutEffect, useMemo, useRef } from "react";
import { Maximize2, Mic, Square, X } from "lucide-react";

import type { LookupOrigin } from "../app/app-types";
import { livePartialHasSubtitle, useLivePartial, useTranslationPartials } from "../realtime-state";
import type { Subtitle } from "../subtitles/types";
import { contentLanguageTag } from "../app/ui-language";
import { compactLivePreview, compactPreviewText } from "../compact-mode";
import { useBatchedPreview } from "../streaming-preview";

export function CompactView({ subtitles, subtitleHistory, subtitleLimit, selectionActive, running, vrchatMuted, captureDisabled, onSelect, onCapture, onRestore, onClose, onResize }: {
  subtitles: Subtitle[];
  subtitleHistory: Subtitle[];
  subtitleLimit: number;
  selectionActive: boolean;
  running: boolean;
  vrchatMuted: boolean;
  captureDisabled: boolean;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onCapture: () => void;
  onRestore: () => void;
  onClose: () => void;
  onResize: () => void;
}) {
  const { t } = useTranslation();
  const microphonePartial = useLivePartial("microphone");
  const speakerPartial = useLivePartial("speaker");
  const rawPartial = selectionActive
    ? null
    : microphonePartial ?? speakerPartial;
  const resolvedPartial = useMemo(
    () => compactLivePreview(rawPartial, subtitleHistory),
    [rawPartial, subtitleHistory],
  );
  const partial = useBatchedPreview(
    resolvedPartial,
    rawPartial?.utterance_id ?? "",
    rawPartial?.utterance_id.startsWith("qwen-preview-") ?? false,
  );
  const historyLimit = Math.max(0, subtitleLimit - (partial ? 1 : 0));
  const visibleSubtitles = historyLimit > 0
    ? subtitles.filter((subtitle) => !partial || !livePartialHasSubtitle(partial, [subtitle])).slice(-historyLimit)
    : [];
  const latestSubtitle = subtitles.at(-1);
  const statusLabel = vrchatMuted ? t("status.pausedVrchatMuted") : partial?.language?.toUpperCase() ?? latestSubtitle?.language?.toUpperCase() ?? "AUTO";
  const captureLabel = t(running ? "capture.pause" : "capture.start");
  return (
    <div className="compact-shell">
      <header className="compact-header" data-tauri-drag-region>
        <div className={`compact-status ${running ? "running" : ""} ${vrchatMuted ? "muted" : ""}`} title={statusLabel} data-tauri-drag-region>
          <i aria-hidden="true" data-tauri-drag-region />
          <span data-tauri-drag-region>{statusLabel}</span>
        </div>
        <div className="compact-window-actions">
          <button type="button" aria-label={t("window.restore")} title={t("window.restore")} onClick={onRestore}><Maximize2 size={15} /></button>
          <button className="compact-close-button" type="button" aria-label={t("window.close")} title={t("window.close")} onClick={onClose}><X size={15} /></button>
        </div>
      </header>
      <div className="compact-body">
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
            <div className={`compact-subtitle-row compact-subtitle-current ${partial.translation ? "compact-subtitle-bilingual" : ""}`}>
              <CompactText
                className="compact-original"
                lang={contentLanguageTag(partial.language)}
                text={partial.text}
                onMouseUp={() => void onSelect(partial.text)}
              />
              {partial.translation && <CompactText className="compact-translation" lang={contentLanguageTag(partial.target_language)} text={partial.translation} />}
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
        </div>
      </div>
      <div className="compact-resize-grip" title={t("window.resize")} onPointerDown={(event) => {
        if (event.button !== 0 || !event.isPrimary) return;
        event.preventDefault();
        event.stopPropagation();
        onResize();
      }}>
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <circle cx="12" cy="4" r="1" /><circle cx="8" cy="8" r="1" /><circle cx="12" cy="8" r="1" />
          <circle cx="4" cy="12" r="1" /><circle cx="8" cy="12" r="1" /><circle cx="12" cy="12" r="1" />
        </svg>
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
  const pendingTranslation = translationPartial ?? subtitle.translation_partial;
  const preview = useBatchedPreview(
    pendingTranslation ?? null,
    `${subtitle.utterance_id ?? subtitle.id}:${pendingTranslation?.target_language ?? ""}`,
    subtitle.utterance_id?.startsWith("qwen-source-") ?? false,
  );
  const visibleTranslation = preview ?? subtitle.translations[0];
  const origin: LookupOrigin = {
    id: subtitle.id,
    language: subtitle.language,
    source: subtitle.source ?? null,
    createdAt: subtitle.created_at,
    translation: visibleTranslation?.text ?? null,
  };

  return (
    <div className={`compact-subtitle-row ${current ? "compact-subtitle-current" : "compact-subtitle-history"} ${visibleTranslation ? "compact-subtitle-bilingual" : ""}`}>
      <CompactText
        className="compact-original"
        lang={contentLanguageTag(subtitle.language)}
        text={subtitle.text}
        onMouseUp={() => void onSelect(subtitle.text, origin)}
      />
      {visibleTranslation && (
        <CompactText
          className="compact-translation"
          lang={contentLanguageTag(visibleTranslation.target_language)}
          text={visibleTranslation.text}
          streaming={Boolean(preview)}
        />
      )}
    </div>
  );
}

function CompactText({ className, lang, text, streaming = false, onMouseUp }: {
  className: string;
  lang?: string;
  text: string;
  streaming?: boolean;
  onMouseUp?: () => void;
}) {
  const ref = useRef<HTMLParagraphElement>(null);
  const visibleText = compactPreviewText(text);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const followTail = () => {
      const selection = window.getSelection();
      if (selection && !selection.isCollapsed
        && (element.contains(selection.anchorNode) || element.contains(selection.focusNode))) return;
      element.scrollTop = element.scrollHeight;
      element.scrollLeft = 0;
    };
    followTail();
    const observer = new ResizeObserver(followTail);
    observer.observe(element);
    return () => observer.disconnect();
  }, [visibleText, streaming]);

  return (
    <p ref={ref} className={className} lang={lang} onMouseUp={onMouseUp}>
      {visibleText}
      {streaming && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
    </p>
  );
}
