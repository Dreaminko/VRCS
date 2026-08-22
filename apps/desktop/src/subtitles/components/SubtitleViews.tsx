import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, RefObject } from "react";
import { createPortal } from "react-dom";
import { LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { LookupOrigin } from "../../app/app-types";
import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../../app/interface-scale";
import type { SubtitleAnalysisOutcome } from "../../subtitle-actions";
import type { Subtitle } from "../types";
import { usePrependScrollAnchor } from "../hooks/usePrependScrollAnchor";
import {
  useSubtitleBoxSelection,
  type SubtitleSelectionRect,
} from "../hooks/useSubtitleBoxSelection";
import { EmptyLiveView, LivePartials } from "./LivePartials";
import { SubtitleBubble } from "./SubtitleBubble";

export { TopStatus } from "./TopStatus";

export const LiveView = memo(function LiveView({
  subtitles,
  scrollContainerRef,
  running,
  hasOlder,
  loading,
  loadingOlder,
  onLoadOlder,
  onSelect,
  onTranslate,
  onAddLearning,
  onOpenLearning,
  onAnalyzeSentence,
  onAddLearningSelection,
  onOpenLearningSelection,
  onAnalyzeSelection,
  onOpenLearningItem,
  isLearningBusy,
  isLearningCaptured,
  isLearningSelectionBusy,
  isLearningSelectionCaptured,
  translatingSubtitleIds = [],
}: {
  subtitles: Subtitle[];
  scrollContainerRef: RefObject<HTMLDivElement | null>;
  running: boolean;
  hasOlder: boolean;
  loading: boolean;
  loadingOlder: boolean;
  onLoadOlder: () => Promise<void>;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  onAddLearning?: (subtitle: Subtitle) => Promise<unknown>;
  onOpenLearning?: (subtitle: Subtitle) => Promise<unknown>;
  onAnalyzeSentence?: (subtitle: Subtitle) => Promise<SubtitleAnalysisOutcome | null>;
  onAddLearningSelection?: (subtitles: Subtitle[]) => Promise<unknown>;
  onOpenLearningSelection?: (subtitles: Subtitle[]) => Promise<unknown>;
  onAnalyzeSelection?: (subtitles: Subtitle[]) => Promise<SubtitleAnalysisOutcome | null>;
  onOpenLearningItem?: (itemId: number) => void;
  isLearningBusy?: (subtitle: Subtitle) => boolean;
  isLearningCaptured?: (subtitle: Subtitle) => boolean;
  isLearningSelectionBusy?: (subtitles: Subtitle[]) => boolean;
  isLearningSelectionCaptured?: (subtitles: Subtitle[]) => boolean;
  translatingSubtitleIds?: number[];
}) {
  const { t } = useTranslation();
  const chronological = useMemo(() => [...subtitles].reverse(), [subtitles]);
  const selectableIds = useMemo(
    () => chronological.flatMap((subtitle) => subtitle.id === null ? [] : [subtitle.id]),
    [chronological],
  );
  const {
    containerRef,
    selectedIds,
    dragRect,
    selecting,
    clearSelection,
    pointerHandlers,
  } = useSubtitleBoxSelection(selectableIds);
  const prependWithScrollAnchor = usePrependScrollAnchor(
    loadingOlder,
    scrollContainerRef,
    containerRef,
  );
  const translatingIds = useMemo(
    () => new Set(translatingSubtitleIds),
    [translatingSubtitleIds],
  );
  const selectedSubtitles = useMemo(
    () => chronological.filter((subtitle) => subtitle.id !== null && selectedIds.has(subtitle.id)),
    [chronological, selectedIds],
  );
  const multiSelectionActive = selectedSubtitles.length > 1;
  const selectionLearningBusy = isLearningSelectionBusy?.(selectedSubtitles) ?? false;
  const selectionLearningCaptured = isLearningSelectionCaptured?.(selectedSubtitles) ?? false;
  const feedbackTimerRef = useRef<number | null>(null);
  const feedbackSequenceRef = useRef(0);
  const [copyFeedback, setCopyFeedback] = useState<{
    id: number;
    tone: "success" | "error";
    text: string;
  } | null>(null);

  useEffect(() => () => {
    if (feedbackTimerRef.current !== null) window.clearTimeout(feedbackTimerRef.current);
  }, []);

  useEffect(() => {
    if (multiSelectionActive) window.getSelection()?.removeAllRanges();
  }, [multiSelectionActive]);

  const showCopyFeedback = useCallback((tone: "success" | "error", text: string) => {
    if (feedbackTimerRef.current !== null) window.clearTimeout(feedbackTimerRef.current);
    feedbackSequenceRef.current += 1;
    setCopyFeedback({ id: feedbackSequenceRef.current, tone, text });
    feedbackTimerRef.current = window.setTimeout(() => {
      setCopyFeedback(null);
      feedbackTimerRef.current = null;
    }, 2_200);
  }, []);

  const loadOlderMessages = useCallback(async () => {
    if (loadingOlder) return;
    await prependWithScrollAnchor(onLoadOlder);
  }, [loadingOlder, onLoadOlder, prependWithScrollAnchor]);

  return (
    <section
      ref={containerRef}
      className={`conversation ${selecting ? "box-selecting" : ""} ${multiSelectionActive ? "multi-selection-active" : ""}`}
      aria-label={t("live.title")}
      {...pointerHandlers}
    >
      {dragRect && createPortal(
        <div className="subtitle-selection-rect" style={selectionOverlayStyle(dragRect)} aria-hidden="true" />,
        document.body,
      )}
      {copyFeedback && (
        <div key={copyFeedback.id} className={`subtitle-copy-toast ${copyFeedback.tone}`} role="status">
          {copyFeedback.text}
        </div>
      )}
      {chronological.length ? (
        <>
          {hasOlder && (
            <div className="conversation-load-older">
              <button
                className="secondary-button"
                type="button"
                disabled={loadingOlder}
                onClick={() => void loadOlderMessages()}
              >
                {loadingOlder && <LoaderCircle className="spinning" size={14} />}
                {t(loadingOlder
                  ? "conversations.loadingOlderMessages"
                  : "conversations.loadOlderMessages")}
              </button>
            </div>
          )}
          {chronological.map((subtitle, index) => {
            const selected = subtitle.id !== null && selectedIds.has(subtitle.id);
            return (
              <SubtitleBubble
                key={subtitle.id ?? `${subtitle.created_at}-${index}`}
                subtitle={subtitle}
                selected={selected}
                selectionActive={selectedIds.size > 0}
                selection={selected && selectedSubtitles.length > 1 ? selectedSubtitles : null}
                onClearSelection={clearSelection}
                onSelect={onSelect}
                onTranslate={onTranslate}
                onAddLearning={onAddLearning}
                onOpenLearning={onOpenLearning}
                onAnalyzeSentence={onAnalyzeSentence}
                onAddLearningSelection={onAddLearningSelection}
                onOpenLearningSelection={onOpenLearningSelection}
                onAnalyzeSelection={onAnalyzeSelection}
                onOpenLearningItem={onOpenLearningItem}
                onCopyFeedback={showCopyFeedback}
                learningBusy={isLearningBusy?.(subtitle)}
                learningCaptured={isLearningCaptured?.(subtitle)}
                selectionLearningBusy={selectionLearningBusy}
                selectionLearningCaptured={selectionLearningCaptured}
                translating={subtitle.id !== null && translatingIds.has(subtitle.id)}
              />
            );
          })}
        </>
      ) : loading ? (
        <div className="empty-state" role="status">
          <LoaderCircle className="spinning" size={22} />
          <p>{t("common.loading")}</p>
        </div>
      ) : (
        <EmptyLiveView running={running} />
      )}
      {running && <LivePartials />}
    </section>
  );
});

function selectionOverlayStyle(rect: SubtitleSelectionRect): CSSProperties {
  const scale = readAppliedInterfaceScaleFactor();
  return {
    top: interfaceLayoutPixels(rect.top, scale),
    left: interfaceLayoutPixels(rect.left, scale),
    width: interfaceLayoutPixels(rect.width, scale),
    height: interfaceLayoutPixels(rect.height, scale),
  };
}
