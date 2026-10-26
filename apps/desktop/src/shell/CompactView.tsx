import { useTranslation } from "react-i18next";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type Ref } from "react";
import { Check, Maximize2, MessageSquare, Mic, Square, X } from "lucide-react";

import type { LookupOrigin, SelectionTarget } from "../app/app-types";
import type { LiveTranscription } from "../capture/types";
import { useLivePartials } from "../core-client/realtime-state";
import type { SpeakerIdentity, Subtitle } from "../subtitles/types";
import { contentLanguageTag } from "../app/ui-language";
import { useSpeakerIdentity } from "../subtitles/speaker-state";
import { livePartialVisible, subtitleVisible } from "../subtitles/player-history";
import { selectPlayerHistory, selectPlayerPreviews } from "./player-previews";
import { compactLivePreview } from "./compact-mode";
import { useBatchedPreview } from "../shared/hooks/streaming-preview";
import { playerCaptionMinimumHeight, type CompactFontSize, type CompactTextBudget } from "./compact-layout";
import { useCompactLayout, type CompactGroup } from "./useCompactLayout";
import { CompactBackgroundControls, CompactWindowIcon } from "./CompactBackgroundControls";

const EMPTY_PREVIEWS: LiveTranscription[] = [];
const EMPTY_FILTER: readonly string[] = [];

export function CompactView({ transparentBackground, onToggleBackground, backgroundOpacity, onBackgroundOpacityChange, fontSize, onFontSizeChange, subtitles, subtitleHistory, selectionTarget, playerLimit, hiddenTracks = EMPTY_FILTER, hiddenSourceLanguages = EMPTY_FILTER, onCaptionHeight, running, vrchatMuted, captureDisabled, chatboxOpen, chatboxDisabled, chatboxButtonRef, onChatbox, onSelect, onCapture, onRestore, onClose, onResize }: {
  transparentBackground: boolean;
  onToggleBackground: () => void;
  backgroundOpacity: number;
  onBackgroundOpacityChange: (value: number) => void;
  fontSize: CompactFontSize;
  onFontSizeChange: (size: CompactFontSize) => void;
  subtitles: Subtitle[];
  subtitleHistory: Subtitle[];
  selectionTarget: SelectionTarget | null;
  playerLimit?: number;
  hiddenTracks?: readonly string[];
  hiddenSourceLanguages?: readonly string[];
  onCaptionHeight?: (minimum: number) => Promise<void> | void;
  running: boolean;
  vrchatMuted: boolean;
  captureDisabled: boolean;
  chatboxOpen?: boolean;
  chatboxDisabled?: boolean;
  chatboxButtonRef?: Ref<HTMLButtonElement>;
  onChatbox?: () => void;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onCapture: () => void;
  onRestore: () => void;
  onClose: () => void;
  onResize: () => void;
}) {
  const { t } = useTranslation();
  const stackRef = useRef<HTMLDivElement>(null);
  const microphonePartials = useLivePartials("microphone");
  const speakerPartials = useLivePartials("speaker");
  const previews = useMemo(() => selectPlayerPreviews(
    speakerPartials.map(partial => compactLivePreview(partial, subtitleHistory)!)
      .filter(partial => livePartialVisible(partial, hiddenTracks, hiddenSourceLanguages)),
    microphonePartials.map(partial => compactLivePreview(partial, subtitleHistory)!), playerLimit),
    [speakerPartials, microphonePartials, playerLimit, subtitleHistory, hiddenTracks, hiddenSourceLanguages]);
  const batched = useBatchedPreview(previews, "compact-previews", previews.some(partial => partial.utterance_id.includes("qwen-preview-"))) ?? EMPTY_PREVIEWS;
  // A hidden/currently filtered lane must disappear immediately, even while a
  // previous Qwen snapshot is waiting for its next display batch.
  const visiblePreviews = useMemo(() => batched.filter(partial => previews.some(current =>
    current.source === partial.source && current.utterance_id === partial.utterance_id)), [batched, previews]);
  const visibleSubtitles = useMemo(() => selectPlayerHistory(
    subtitles.filter(subtitle => subtitleVisible(subtitle, hiddenTracks, hiddenSourceLanguages)), previews, playerLimit),
    [subtitles, previews, playerLimit, hiddenTracks, hiddenSourceLanguages]);
  const layout = useCompactLayout({ subtitles: visibleSubtitles, previews: visiblePreviews, fontSize, selectionTarget, transparentBackground, perPlayer: playerLimit !== undefined });
  const captionTracks = layout.visibleGroups.filter(group => group.source !== "chatbox").length;
  const minimumRows = captionTracks + Number(layout.visibleGroups.some(group => group.source === "chatbox"));
  useEffect(() => {
    if (layout.parallel && !layout.frozen) {
      void onCaptionHeight?.(playerCaptionMinimumHeight(minimumRows, fontSize));
    }
  }, [layout.parallel, layout.frozen, minimumRows, fontSize, onCaptionHeight]);
  const latest = layout.visibleGroups.at(-1);
  useLayoutEffect(() => {
    const stack = stackRef.current;
    if (!layout.scrollable || layout.frozen || !stack) return;
    let following = true;
    const followTail = () => { if (following) stack.scrollTop = stack.scrollHeight; };
    const trackScroll = () => { following = stack.scrollHeight - stack.scrollTop - stack.clientHeight <= 1; };
    followTail();
    const observer = new ResizeObserver(followTail);
    observer.observe(stack);
    for (const row of stack.children) observer.observe(row);
    stack.addEventListener("scroll", trackScroll);
    return () => {
      observer.disconnect();
      stack.removeEventListener("scroll", trackScroll);
    };
  }, [layout.scrollable, layout.frozen, latest?.key]);
  const statusLabel = vrchatMuted ? t("status.pausedVrchatMuted") : latest?.language?.toUpperCase() ?? "AUTO";
  const captureLabel = t(running ? "capture.pause" : "capture.start");
  return (
    <div className="compact-shell">
      <header className="compact-header" data-tauri-drag-region>
        <div className={`compact-status ${running ? "running" : ""} ${vrchatMuted ? "muted" : ""}`} data-tooltip={statusLabel} data-tauri-drag-region>
          <i aria-hidden="true" data-tauri-drag-region />
          <span data-tauri-drag-region>{statusLabel}</span>
        </div>
        <div className="compact-window-actions">
          {onChatbox && <button ref={chatboxButtonRef} className="compact-chatbox-toggle" type="button"
            aria-label={t("chatbox.title")} data-tooltip={t("chatbox.title")} aria-expanded={chatboxOpen}
            aria-controls={chatboxOpen ? "compact-chatbox" : undefined} disabled={chatboxDisabled} onClick={onChatbox}>
            <CompactWindowIcon icon={MessageSquare} />
          </button>}
          <CompactFontMenu value={fontSize} onChange={onFontSizeChange} />
          <CompactBackgroundControls transparentBackground={transparentBackground} onToggleBackground={onToggleBackground}
            opacity={backgroundOpacity} onOpacityChange={onBackgroundOpacityChange} />
          <button type="button" aria-label={t("window.restore")} data-tooltip={t("window.restore")} onClick={onRestore}><CompactWindowIcon icon={Maximize2} /></button>
          <button className="compact-close-button" type="button" aria-label={t("window.close")} data-tooltip={t("window.close")} onClick={onClose}><CompactWindowIcon icon={X} /></button>
        </div>
      </header>
      <div className="compact-body">
        <div ref={layout.contentRef} className="compact-content">
          <div ref={stackRef} className={`compact-subtitle-stack${layout.parallel ? " compact-parallel" : ""}${layout.scrollable ? " compact-scrollable" : ""}`}>
            {layout.visibleGroups.map((group, index) => (
              <CompactSubtitleRow key={group.key} group={group} current={(Boolean(layout.parallel) && group.source !== "chatbox") || index === layout.visibleGroups.length - 1}
                budget={layout.parallel || index === layout.visibleGroups.length - 1 ? layout.latestBudget : undefined}
                frozen={layout.frozen} onSelect={onSelect} />
            ))}
            {layout.visibleGroups.length === 0 && (
              <div className="compact-subtitle-row compact-subtitle-current">
                <p className="compact-original">{t("live.waiting")}</p>
              </div>
            )}
          </div>
          <div ref={layout.measureRef} className="compact-measurements" aria-hidden="true" inert>
            {layout.measurementGroups.map((group, index) => (
              <div key={group.key} className={`compact-subtitle-row ${index === layout.measurementGroups.length - 1 ? "compact-subtitle-current" : "compact-subtitle-history"}`}>
                <CompactSpeaker speaker={group.speaker} source={group.source} />
                {group.text && <p className="compact-original" lang={contentLanguageTag(group.language)}>{group.text}</p>}
                {group.translation && <p className="compact-translation" lang={contentLanguageTag(group.targetLanguage)}>{group.translation}{group.streaming && <span className="streaming-ellipsis">…</span>}</p>}
              </div>
            ))}
          </div>
        </div>
        <div className="compact-actions">
          <button className="compact-capture-button" type="button" aria-label={captureLabel} aria-pressed={running} data-tooltip={captureLabel} disabled={captureDisabled} onClick={onCapture}>
            {running ? <Square size={15} /> : <Mic size={16} />}
          </button>
        </div>
      </div>
      <div className="compact-resize-grip" data-tooltip={t("window.resize")} onPointerDown={(event) => {
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

const FONT_SIZES: CompactFontSize[] = ["small", "medium", "large"];
const FONT_LABELS = { small: "window.subtitleFontSizeSmall", medium: "window.subtitleFontSizeMedium", large: "window.subtitleFontSizeLarge" } as const;

function CompactFontMenu({ value, onChange }: { value: CompactFontSize; onChange: (size: CompactFontSize) => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const controlRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const label = t("window.subtitleFontSizeCurrent", { size: t(FONT_LABELS[value]) });
  const close = () => { setOpen(false); buttonRef.current?.focus({ preventScroll: true }); };

  useLayoutEffect(() => {
    if (open) menuRef.current?.querySelector<HTMLButtonElement>(`[data-size="${value}"]`)?.focus({ preventScroll: true });
  }, [open, value]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!controlRef.current?.contains(event.target as Node)) {
        setOpen(false);
        buttonRef.current?.focus({ preventScroll: true });
      }
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      buttonRef.current?.focus({ preventScroll: true });
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape, true);
    };
  }, [open]);

  return (
    <div ref={controlRef} className="compact-font-control" onBlur={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
    }}>
      <button ref={buttonRef} type="button" className="compact-font-button" aria-label={label} data-tooltip={label}
        aria-haspopup="menu" aria-expanded={open} aria-controls={open ? "compact-font-menu" : undefined}
        onClick={() => setOpen((previous) => !previous)} onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setOpen(true); }
        }}>Aa</button>
      {open && <div ref={menuRef} id="compact-font-menu" className="compact-font-menu" role="menu" aria-label={t("window.subtitleFontSize")}
        onKeyDown={(event) => {
          const options = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>("button") ?? []);
          const index = options.indexOf(document.activeElement as HTMLButtonElement);
          let next: number;
          if (event.key === "ArrowDown") next = (index + 1) % options.length;
          else if (event.key === "ArrowUp") next = (index + options.length - 1) % options.length;
          else if (event.key === "Home") next = 0;
          else if (event.key === "End") next = options.length - 1;
          else if (event.key === "Tab") { event.preventDefault(); close(); return; }
          else return;
          event.preventDefault();
          options[next]?.focus();
        }}>
        {FONT_SIZES.map((size) => <button key={size} data-size={size} type="button" role="menuitemradio" aria-checked={value === size}
          tabIndex={-1} onClick={() => { onChange(size); close(); }}>
          <span>{t(FONT_LABELS[size])}</span>
          {value === size && <Check size={14} aria-hidden="true" />}
        </button>)}
      </div>}
    </div>
  );
}

function CompactSpeaker({ speaker: identity, source }: { speaker?: SpeakerIdentity | null; source?: string }) {
  const { t } = useTranslation();
  const speaker = useSpeakerIdentity(identity);
  return <div className="compact-speaker">{source === "chatbox" ? t("chatbox.title") : speaker?.display_name || (source === "microphone" ? t("live.microphoneMe")
    : speaker ? t("live.trackNumber", { number: speaker.index + 1 }) : t("live.speakerOther"))}</div>;
}

function CompactSubtitleRow({ group, current, budget, frozen, onSelect }: {
  group: CompactGroup;
  current: boolean;
  budget?: CompactTextBudget;
  frozen: boolean;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
}) {
  return (
    <div className={`compact-subtitle-row ${current ? "compact-subtitle-current" : "compact-subtitle-history"}`}>
      <CompactSpeaker speaker={group.speaker} source={group.source} />
      {group.text && <CompactText className="compact-original" lang={contentLanguageTag(group.language)} text={group.text}
        height={budget?.original} frozen={frozen} onMouseUp={() => void onSelect(group.context, group.origin)} />}
      {group.translation && <CompactText className="compact-translation" lang={contentLanguageTag(group.targetLanguage)}
        text={group.translation} streaming={group.streaming} height={budget?.translation} frozen={frozen} />}
    </div>
  );
}

function CompactText({ className, lang, text, streaming = false, height, frozen, onMouseUp }: {
  className: string;
  lang?: string;
  text: string;
  streaming?: boolean;
  height?: number;
  frozen: boolean;
  onMouseUp?: () => void;
}) {
  const ref = useRef<HTMLParagraphElement>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element || frozen) return;
    if (height === undefined) { element.scrollTop = 0; return; }
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
  }, [text, streaming, height, frozen]);

  return (
    <p ref={ref} className={className} lang={lang} onMouseUp={onMouseUp} style={height === undefined ? undefined : { height }}>
      {text}{streaming && <span className="streaming-ellipsis" aria-hidden="true">…</span>}
    </p>
  );
}
