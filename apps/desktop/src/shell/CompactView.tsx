import { useTranslation } from "react-i18next";
import { useLayoutEffect, useRef } from "react";
import { Maximize2, Mic, Square, X } from "lucide-react";

import type { LookupOrigin } from "../app/app-types";
import { livePartialHasSubtitle, useLivePartial, useTranslationPartials } from "../realtime-state";
import type { Subtitle } from "../subtitles/types";
import { contentLanguageTag } from "../app/ui-language";
import { compactPreviewText } from "../compact-mode";

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
    ? subtitles.filter((subtitle) => !partial || !livePartialHasSubtitle(partial, [subtitle])).slice(-historyLimit)
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
          streaming={Boolean(translationPartial || subtitle.translation_partial)}
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
      element.scrollLeft = getComputedStyle(element).direction === "rtl"
        ? -element.scrollWidth
        : element.scrollWidth;
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
