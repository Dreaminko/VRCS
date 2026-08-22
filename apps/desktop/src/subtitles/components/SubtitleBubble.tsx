import { memo, useEffect, useRef, useState } from "react";
import { Check, LoaderCircle, MessageSquare, Mic, Sparkles, Volume2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { timestamp } from "../../app/app-utils";
import type { LookupOrigin } from "../../app/app-types";
import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../../app/interface-scale";
import { contentLanguageTag } from "../../app/ui-language";
import { useTranslationPartials } from "../../realtime-state";
import {
  subtitleCopyText,
  subtitleSelectionCopyText,
  type SubtitleAnalysisOutcome,
  type SubtitleCopyMode,
} from "../../subtitle-actions";
import type { Subtitle } from "../types";
import { SubtitleAnalysisPopover } from "./SubtitleAnalysisPopover";
import { SubtitleContextMenu } from "./SubtitleContextMenu";

type SubtitleSource = NonNullable<Subtitle["source"]>;

export const SubtitleBubble = memo(function SubtitleBubble({
  subtitle,
  focused,
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
  focused: boolean;
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
  const translationPartials = useTranslationPartials(subtitle.id);
  const completedTranslation = subtitle.translations[0];
  const fallbackPartial = subtitle.translation_partial
    ? [{ ...subtitle.translation_partial, preferred: true }]
    : [];
  const activePartials = translationPartials.length ? translationPartials : fallbackPartial;
  const partialLanguages = new Set(activePartials.map((partial) => partial.target_language));
  const visibleTranslations = [
    ...activePartials,
    ...subtitle.translations.filter(
      (translation) => !partialLanguages.has(translation.target_language),
    ),
  ];
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
    translation: visibleTranslations[0]?.text ?? null,
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
      className={`message-group source-${source} ${selected ? "subtitle-selected" : ""} ${focused ? "subtitle-search-focused" : ""}`}
      aria-selected={selected}
      aria-current={focused ? "true" : undefined}
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
        {visibleTranslations.length > 0 && (
          <>
            <div className="bubble-translation-divider" aria-hidden="true" />
            {visibleTranslations.map((translation) => {
              const streaming = activePartials.some(
                (partial) => partial.target_language === translation.target_language,
              );
              return (
                <p className={`bubble-translation ${streaming ? "streaming-translation" : ""}`} lang={contentLanguageTag(translation.target_language)} key={translation.target_language}>
                  {translation.text}
                  {streaming && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
                </p>
              );
            })}
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
