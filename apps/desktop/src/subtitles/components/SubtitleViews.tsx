import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, RefObject } from "react";
import { createPortal } from "react-dom";
import { Globe2, LoaderCircle, SlidersHorizontal } from "lucide-react";
import { useTranslation } from "react-i18next";
import { subtitleVisible } from "../player-history";
import { useLanguageDetectorReady } from "../usePlayerHistoryPreferences";

import type { LookupOrigin } from "../../app/app-types";
import type { SubtitleAnalysisOutcome } from "../subtitle-actions";
import type { Subtitle } from "../types";
import { usePrependScrollAnchor } from "../hooks/usePrependScrollAnchor";
import {
  useSubtitleBoxSelection,
  type SubtitleSelectionRect,
} from "../hooks/useSubtitleBoxSelection";
import { EmptyLiveView, LivePartials } from "./LivePartials";
import { SubtitleBubble } from "./SubtitleBubble";

export { TopStatus } from "./TopStatus";

const EMPTY_SUBTITLE_SELECTION: Subtitle[] = [];
const EMPTY_NUMBER_SET = new Set<number>();
const EMPTY_TRANSLATING_IDS: number[] = [];
const EMPTY_HIDDEN_TRACKS: readonly string[] = [];

export const LiveView = memo(function LiveView({
  subtitles,
  playerName,
  hiddenTracks = EMPTY_HIDDEN_TRACKS,
  hiddenSourceLanguages,
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
  translatingSubtitleIds = EMPTY_TRANSLATING_IDS,
  focusedSubtitleId = null,
}: {
  subtitles: Subtitle[];
  playerName?: string;
  hiddenTracks?: readonly string[];
  hiddenSourceLanguages?: readonly string[];
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
  focusedSubtitleId?: number | null;
}) {
  const { t } = useTranslation();
  const languageDetectorReady = useLanguageDetectorReady();
  const chronological = useMemo(() => {
    const visible: Subtitle[] = [];
    for (let index = subtitles.length - 1; index >= 0; index -= 1) {
      const subtitle = subtitles[index];
      if (subtitleVisible(subtitle, hiddenTracks, hiddenSourceLanguages)) visible.push(subtitle);
    }
    return visible;
  }, [subtitles, hiddenTracks, hiddenSourceLanguages, languageDetectorReady]);
  const selectableIds = useMemo(
    () => chronological.flatMap((subtitle) => (
      subtitle.id === null || subtitle.source === "world" ? [] : [subtitle.id]
    )),
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
    () => (translatingSubtitleIds.length ? new Set(translatingSubtitleIds) : EMPTY_NUMBER_SET),
    [translatingSubtitleIds],
  );
  const selectedSubtitles = useMemo(
    () => (selectedIds.size > 1
      ? chronological.filter((subtitle) => subtitle.id !== null && selectedIds.has(subtitle.id))
      : EMPTY_SUBTITLE_SELECTION),
    [chronological, selectedIds],
  );
  const multiSelectionActive = selectedSubtitles.length > 1;
  const selectionLearningBusy = multiSelectionActive
    ? (isLearningSelectionBusy?.(selectedSubtitles) ?? false)
    : false;
  const selectionLearningCaptured = multiSelectionActive
    ? (isLearningSelectionCaptured?.(selectedSubtitles) ?? false)
    : false;
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

  useLayoutEffect(() => {
    if (focusedSubtitleId === null || loading) return;
    containerRef.current
      ?.querySelector<HTMLElement>(`[data-subtitle-id="${focusedSubtitleId}"]`)
      ?.scrollIntoView({ block: "center" });
  }, [containerRef, focusedSubtitleId, loading]);

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
      {chronological.length ? (
        <>
          {chronological.map((subtitle, index) => {
            if (subtitle.source === "world") {
              return (
                <div
                  key={subtitle.id ?? `${subtitle.created_at}-${index}`}
                  className={`world-marker ${subtitle.id === focusedSubtitleId ? "subtitle-search-focused" : ""}`}
                  data-subtitle-id={subtitle.id ?? undefined}
                >
                  <span className="world-marker-line" aria-hidden="true" />
                  <span className="world-marker-label">
                    <Globe2 size={14} aria-hidden="true" />
                    {t("live.enteredWorld", { name: subtitle.text })}
                  </span>
                  <span className="world-marker-line" aria-hidden="true" />
                </div>
              );
            }
            const selected = subtitle.id !== null && selectedIds.has(subtitle.id);
            return (
              <SubtitleBubble
                key={subtitle.id ?? `${subtitle.created_at}-${index}`}
                subtitle={subtitle}
                playerName={playerName}
                focused={subtitle.id === focusedSubtitleId}
                selected={selected}
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
      ) : subtitles.length ? (
        <div className="empty-state">
          <SlidersHorizontal size={22} aria-hidden="true" />
          <p>{t("live.players.filteredEmpty")}</p>
        </div>
      ) : (
        <EmptyLiveView running={running} />
      )}
      {running && <LivePartials subtitles={subtitles} playerName={playerName} hiddenTracks={hiddenTracks} hiddenSourceLanguages={hiddenSourceLanguages} />}
    </section>
  );
});

function selectionOverlayStyle(rect: SubtitleSelectionRect): CSSProperties {
  return {
    top: rect.top,
    left: rect.left,
    width: rect.width,
    height: rect.height,
  };
}
