import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, RefObject } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Check, LoaderCircle, MessageSquare, Mic, Sparkles, Volume2 } from "lucide-react";

import { timestamp } from "../app-utils";
import type { LookupOrigin } from "../app-types";
import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../interface-scale";
import { useLivePartial, useTranslationPartial } from "../realtime-state";
import {
  subtitleCopyText,
  subtitleSelectionCopyText,
  type SubtitleAnalysisOutcome,
  type SubtitleCopyMode,
} from "../subtitle-actions";
import { usePrependScrollAnchor } from "../hooks/usePrependScrollAnchor";
import {
  useSubtitleBoxSelection,
  type SubtitleSelectionRect,
} from "../hooks/useSubtitleBoxSelection";

import { contentLanguageTag } from "../ui-language";
import type { ConnectionState, Health, Settings, Subtitle } from "../types";
import { SubtitleAnalysisPopover } from "./SubtitleAnalysisPopover";
import { SubtitleContextMenu } from "./SubtitleContextMenu";

type SubtitleSource = NonNullable<Subtitle["source"]>;

export function TopStatus({ connection, health, settings }: {
  connection: ConnectionState;
  health: Health | null;
  settings: Settings | null;
}) {
  const { t } = useTranslation();
  const connectionLabel = t(`status.connection.${connection}`);
  return (
    <div className="top-status-row">
      <div className="status-summary" aria-label={t("status.summary")}>
        <div className={`core-summary connection-${connection}`}><span>Core</span><strong><i aria-hidden="true" />{connectionLabel}</strong></div>
        <i aria-hidden="true" />
        <div><span>{t("status.label")}</span><strong>{transcriptionStatusLabel(health, t)}</strong></div>
        <i aria-hidden="true" />
        <div className={health?.osc?.send_gate === "open" ? "mute-summary" : "mute-summary muted"}>
          <span>{t("status.vrchat")}</span>
          <strong>{vrchatSendStatusLabel(health, settings, t)}</strong>
        </div>
        <i aria-hidden="true" />
        <div><span>{t("status.engine")}</span><strong>{engineLabel(settings)}</strong></div>
      </div>
    </div>
  );
}

function transcriptionStatusLabel(health: Health | null, t: (key: string) => string): string {
  if (!health?.capture_requested) return t("status.waiting");
  if (health.microphone_capture_state === "paused_vrchat_muted") {
    return t("status.microphonePaused");
  }
  return t("status.transcribing");
}

function vrchatSendStatusLabel(
  health: Health | null,
  settings: Settings | null,
  t: (key: string) => string,
): string {
  if (!settings?.osc.enabled || health?.osc?.status === "disabled") return t("status.vrchatSendDisabled");
  if (!health?.osc) return t("status.vrchatSendChecking");
  if (health?.osc?.status === "error") return t("status.vrchatSendError");
  if (health.osc.send_gate === "blocked_vrchat_muted") {
    return t("status.pausedVrchatMuted");
  }
  if (health.osc.send_gate === "blocked_mute_unknown") return t("status.muteUnknown");
  return t("status.sendReady");
}

function engineLabel(settings: Settings | null): string {
  if (settings?.asr.backend === "qwen_realtime") return "Qwen3 ASR";
  if (settings?.asr.backend === "fun_asr_realtime") return "Fun-ASR";
  if (settings?.asr.backend === "openai_realtime") return "OpenAI Realtime";
  return `Whisper ${capitalize(settings?.asr.local.model ?? "small")}`;
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

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
  const [copyFeedback, setCopyFeedback] = useState<{ id: number; tone: "success" | "error"; text: string } | null>(null);

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
          <ChatBubble
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
        <div className="empty-state" role="status"><LoaderCircle className="spinning" size={22} /><p>{t("common.loading")}</p></div>
      ) : (
        <div className="empty-state"><MessageSquare size={22} /><p>{running ? t("live.listening") : t("live.startHint")}</p></div>
      )}
      {running && <LivePartials />}
    </section>
  );
});

function LivePartials() {
  const { t } = useTranslation();
  const speaker = useLivePartial("speaker");
  const microphone = useLivePartial("microphone");
  const partials = [speaker, microphone].flatMap((partial) => partial ? [partial] : []);
  if (!partials.length) {
    return (
      <div className="message-group source-speaker streaming-message">
        <div className="bubble">{t("live.transcribing")}<span className="streaming-ellipsis" aria-hidden="true">…</span></div>
      </div>
    );
  }
  return partials.map((partial) => (
    <div className={`message-group source-${partial.source} streaming-message`} key={`${partial.source}-${partial.utterance_id}`}>
      <div className="bubble" lang={contentLanguageTag(partial.language)}>{partial.text}<span className="streaming-ellipsis" aria-hidden="true">…</span></div>
    </div>
  ));
}

const ChatBubble = memo(function ChatBubble({
  subtitle,
  selected,
  selectionActive,
  selection,
  onClearSelection,
  onSelect,
  onTranslate,
  onAddLearning,
  onOpenLearning,
  onAnalyzeSentence,
  onAddLearningSelection,
  onOpenLearningSelection,
  onAnalyzeSelection,
  onOpenLearningItem,
  onCopyFeedback,
  translating = false,
  learningBusy = false,
  learningCaptured = false,
  selectionLearningBusy = false,
  selectionLearningCaptured = false,
}: {
  subtitle: Subtitle;
  selected: boolean;
  selectionActive: boolean;
  selection: Subtitle[] | null;
  onClearSelection: () => void;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  onAddLearning?: (subtitle: Subtitle) => Promise<unknown>;
  onOpenLearning?: (subtitle: Subtitle) => Promise<unknown>;
  onAnalyzeSentence?: (subtitle: Subtitle) => Promise<SubtitleAnalysisOutcome | null>;
  onAddLearningSelection?: (subtitles: Subtitle[]) => Promise<unknown>;
  onOpenLearningSelection?: (subtitles: Subtitle[]) => Promise<unknown>;
  onAnalyzeSelection?: (subtitles: Subtitle[]) => Promise<SubtitleAnalysisOutcome | null>;
  onOpenLearningItem?: (itemId: number) => void;
  onCopyFeedback: (tone: "success" | "error", text: string) => void;
  translating?: boolean;
  learningBusy?: boolean;
  learningCaptured?: boolean;
  selectionLearningBusy?: boolean;
  selectionLearningCaptured?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const source: SubtitleSource = subtitle.source ?? "speaker";
  const mine = source !== "speaker";
  const translationPartial = useTranslationPartial(subtitle.id);
  const completedTranslation = subtitle.translations.at(-1);
  const visibleTranslation = translationPartial ?? subtitle.translation_partial ?? completedTranslation;
  const selectionMode = Boolean(selection?.length);
  const actionTranslation = selectionMode
    ? subtitleSelectionCopyText(selection!, "translation") || null
    : completedTranslation?.text ?? null;
  const actionLearningBusy = selectionMode ? selectionLearningBusy : learningBusy;
  const actionLearningCaptured = selectionMode ? selectionLearningCaptured : learningCaptured;
  const actionTargetKey = selectionMode
    ? `selection:${selection!.map((item) => item.id).join(",")}`
    : `subtitle:${subtitle.id ?? subtitle.created_at}`;
  const articleRef = useRef<HTMLElement>(null);
  const bubbleRef = useRef<HTMLDivElement>(null);
  const [menuPosition, setMenuPosition] = useState<ReturnType<typeof subtitleContextMenuPosition> | null>(null);
  const [selectedText, setSelectedText] = useState("");
  const [analysisState, setAnalysisState] = useState<"idle" | "analyzing" | "completed" | "error">("idle");
  const [analysisResult, setAnalysisResult] = useState<Extract<SubtitleAnalysisOutcome, { status: "completed" }> | null>(null);
  const [analysisPopoverOpen, setAnalysisPopoverOpen] = useState(false);
  const analysisTargetRef = useRef(actionTargetKey);
  analysisTargetRef.current = actionTargetKey;
  const origin: LookupOrigin = {
    id: subtitle.id,
    language: subtitle.language,
    source: subtitle.source ?? null,
    createdAt: subtitle.created_at,
    translation: visibleTranslation?.text ?? null,
  };

  useEffect(() => {
    setMenuPosition(null);
    setAnalysisState("idle");
    setAnalysisResult(null);
    setAnalysisPopoverOpen(false);
  }, [actionTargetKey]);

  const copyText = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      onCopyFeedback("success", t("live.contextMenu.copySuccess"));
    } catch {
      onCopyFeedback("error", t("live.contextMenu.copyFailed"));
    }
  };

  const copySubtitle = (mode: SubtitleCopyMode) => {
    const text = selectionMode
      ? subtitleSelectionCopyText(selection!, mode)
      : subtitleCopyText(subtitle.text, completedTranslation?.text ?? null, mode);
    void copyText(text);
  };

  const analyzeSentence = async () => {
    const analyze = selectionMode
      ? onAnalyzeSelection && (() => onAnalyzeSelection(selection!))
      : onAnalyzeSentence && (() => onAnalyzeSentence(subtitle));
    if (!analyze || analysisState === "analyzing") return;
    const targetKey = actionTargetKey;
    setAnalysisPopoverOpen(false);
    setAnalysisState("analyzing");
    try {
      const outcome = await analyze();
      if (analysisTargetRef.current !== targetKey) return;
      if (!outcome) {
        setAnalysisState("error");
        return;
      }
      if (outcome.status === "completed") {
        setAnalysisResult(outcome);
        setAnalysisState("completed");
        setAnalysisPopoverOpen(true);
      } else {
        setAnalysisState("idle");
      }
    } catch {
      if (analysisTargetRef.current === targetKey) setAnalysisState("error");
    }
  };

  const openContextMenu = (clientX: number, clientY: number) => {
    if (selectionActive && !selected) onClearSelection();
    setAnalysisPopoverOpen(false);
    setSelectedText(selectedTextWithin(articleRef.current));
    setMenuPosition(subtitleContextMenuPosition(clientX, clientY));
  };

  return (
    <article
      ref={articleRef}
      data-subtitle-id={subtitle.id ?? undefined}
      className={`message-group source-${source} ${selected ? "subtitle-selected" : ""}`}
      aria-selected={selected}
      onContextMenu={(event) => {
        event.preventDefault();
        openContextMenu(event.clientX, event.clientY);
      }}
    >
      <div className="message-meta">
        {!mine && <Volume2 size={14} />}
        {mine && <time>{timestamp(subtitle.created_at, locale)}</time>}
        <span>{source === "chatbox" ? t("chatbox.title") : mine ? t("live.microphoneMe") : t("live.speakerOther")}</span>
        {!mine && <time>{timestamp(subtitle.created_at, locale)}</time>}
        {source === "microphone" && <Mic size={14} />}
        {source === "chatbox" && <MessageSquare size={14} />}
      </div>
      <div ref={bubbleRef} className="bubble">
        <p
          className="bubble-original"
          lang={contentLanguageTag(subtitle.language)}
          onMouseUp={(event) => {
            if (event.button === 0) {
              setAnalysisPopoverOpen(false);
              void onSelect(subtitle.text, origin);
            }
          }}
        >{subtitle.text}</p>
        {visibleTranslation && (
          <>
            <div className="bubble-translation-divider" aria-hidden="true" />
            <p className={`bubble-translation ${translationPartial || subtitle.translation_partial ? "streaming-translation" : ""}`} lang={contentLanguageTag(visibleTranslation.target_language)}>
              {visibleTranslation.text}
              {(translationPartial || subtitle.translation_partial) && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
            </p>
          </>
        )}
      </div>
      <div className="subtitle-actions">
        {analysisState === "analyzing" && (
          <span className="subtitle-analysis-feedback analyzing" role="status">
            <LoaderCircle className="spinning" size={13} />{t("live.contextMenu.analyzing")}
          </span>
        )}
        {analysisState === "completed" && analysisResult && (
          <button className="subtitle-analysis-feedback success" type="button" onClick={() => setAnalysisPopoverOpen(true)}>
            <Check size={13} />{t("live.contextMenu.analysisComplete")}
          </button>
        )}
        {analysisState === "error" && (selectionMode ? onAnalyzeSelection : onAnalyzeSentence) && (
          <button className="subtitle-analysis-feedback error" type="button" onClick={() => void analyzeSentence()}>
            <Sparkles size={13} />{t("live.contextMenu.analysisRetry")}
          </button>
        )}
      </div>
      {analysisPopoverOpen && analysisResult && (
        <SubtitleAnalysisPopover
          anchorRef={bubbleRef}
          analysis={analysisResult.analysis}
          onOpenLearning={onOpenLearningItem ? () => onOpenLearningItem(analysisResult.itemId) : undefined}
          onClose={() => setAnalysisPopoverOpen(false)}
        />
      )}
      {menuPosition && (
        <SubtitleContextMenu
          position={menuPosition}
          selectedText={selectedText}
          translation={actionTranslation}
          translating={selectionMode ? false : translating}
          learningBusy={actionLearningBusy}
          learningCaptured={actionLearningCaptured}
          analyzing={analysisState === "analyzing"}
          analysisComplete={analysisState === "completed"}
          onCopySelection={() => void copyText(selectedText)}
          onCopyOriginal={() => copySubtitle("original")}
          onCopyTranslation={() => copySubtitle("translation")}
          onCopyBilingual={() => copySubtitle("bilingual")}
          onTranslate={!selectionMode && onTranslate && subtitle.id !== null ? () => onTranslate(subtitle.id!) : undefined}
          onAnalyze={(selectionMode ? onAnalyzeSelection : onAnalyzeSentence) ? () => void analyzeSentence() : undefined}
          onLearning={selectionMode
            ? (onAddLearningSelection || onOpenLearningSelection)
              ? () => {
                  if (actionLearningCaptured && onOpenLearningSelection) void onOpenLearningSelection(selection!);
                  else if (onAddLearningSelection) void onAddLearningSelection(selection!);
                }
              : undefined
            : subtitle.id !== null && (onAddLearning || onOpenLearning)
              ? () => {
                  if (actionLearningCaptured && onOpenLearning) void onOpenLearning(subtitle);
                  else if (onAddLearning) void onAddLearning(subtitle);
                }
              : undefined}
          selectionCount={selectionMode ? selection!.length : undefined}
          onClearSelection={selectionMode ? onClearSelection : undefined}
          onClose={() => setMenuPosition(null)}
        />
      )}
    </article>
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

function selectedTextWithin(root: HTMLElement | null): string {
  const selection = window.getSelection();
  if (!root || !selection?.anchorNode || !selection.focusNode) return "";
  if (!root.contains(selection.anchorNode) || !root.contains(selection.focusNode)) return "";
  return selection.toString().trim();
}

function subtitleContextMenuPosition(clientX: number, clientY: number) {
  const scale = readAppliedInterfaceScaleFactor();
  const x = interfaceLayoutPixels(clientX, scale);
  const y = interfaceLayoutPixels(clientY, scale);
  const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
  const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
  const width = 236;
  const expectedHeight = 330;
  const gap = 4;
  const side = viewportHeight - y >= expectedHeight ? "below" : "above";
  return {
    top: side === "below" ? y + gap : y - gap,
    left: Math.max(8, Math.min(x + gap, viewportWidth - width - 8)),
    side,
  } as const;
}
