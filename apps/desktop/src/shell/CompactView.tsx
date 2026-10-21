import { useTranslation } from "react-i18next";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Check, createLucideIcon, Maximize2, Mic, Square, X, type LucideIcon } from "lucide-react";

import type { LookupOrigin, SelectionTarget } from "../app/app-types";
import { useLivePartial } from "../core-client/realtime-state";
import type { Subtitle } from "../subtitles/types";
import { contentLanguageTag } from "../app/ui-language";
import { compactLivePreview } from "./compact-mode";
import { useBatchedPreview } from "../shared/hooks/streaming-preview";
import { type CompactFontSize, type CompactTextBudget } from "./compact-layout";
import { useCompactLayout, type CompactGroup } from "./useCompactLayout";

const BackgroundIcon = createLucideIcon("CompactBackground", [
  ["circle", { cx: "12", cy: "12", r: "9", key: "circle" }],
  ["path", { d: "M12 3a9 9 0 0 1 0 18Z", fill: "currentColor", stroke: "none", key: "half" }],
]);

export function CompactView({ transparentBackground, onToggleBackground, fontSize, onFontSizeChange, subtitles, subtitleHistory, selectionTarget, running, vrchatMuted, captureDisabled, onSelect, onCapture, onRestore, onClose, onResize }: {
  transparentBackground: boolean;
  onToggleBackground: () => void;
  fontSize: CompactFontSize;
  onFontSizeChange: (size: CompactFontSize) => void;
  subtitles: Subtitle[];
  subtitleHistory: Subtitle[];
  selectionTarget: SelectionTarget | null;
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
  const rawPartial = microphonePartial ?? speakerPartial;
  const resolvedPartial = useMemo(
    () => compactLivePreview(rawPartial, subtitleHistory),
    [rawPartial, subtitleHistory],
  );
  const partial = useBatchedPreview(
    resolvedPartial,
    rawPartial?.utterance_id ?? "",
    rawPartial?.utterance_id.startsWith("qwen-preview-") ?? false,
  );
  const layout = useCompactLayout({ subtitles, partial, fontSize, selectionTarget, transparentBackground });
  const latest = layout.visibleGroups.at(-1);
  const statusLabel = vrchatMuted ? t("status.pausedVrchatMuted") : latest?.language?.toUpperCase() ?? "AUTO";
  const captureLabel = t(running ? "capture.pause" : "capture.start");
  const backgroundLabel = t(transparentBackground ? "window.restoreBackground" : "window.transparentBackground");
  return (
    <div className="compact-shell">
      <header className="compact-header" data-tauri-drag-region>
        <div className={`compact-status ${running ? "running" : ""} ${vrchatMuted ? "muted" : ""}`} title={statusLabel} data-tauri-drag-region>
          <i aria-hidden="true" data-tauri-drag-region />
          <span data-tauri-drag-region>{statusLabel}</span>
        </div>
        <div className="compact-window-actions">
          <CompactFontMenu value={fontSize} onChange={onFontSizeChange} />
          <button type="button" aria-label={backgroundLabel} aria-pressed={transparentBackground} title={backgroundLabel} onClick={onToggleBackground}><CompactWindowIcon icon={BackgroundIcon} /></button>
          <button type="button" aria-label={t("window.restore")} title={t("window.restore")} onClick={onRestore}><CompactWindowIcon icon={Maximize2} /></button>
          <button className="compact-close-button" type="button" aria-label={t("window.close")} title={t("window.close")} onClick={onClose}><CompactWindowIcon icon={X} /></button>
        </div>
      </header>
      <div className="compact-body">
        <div ref={layout.contentRef} className="compact-content">
          <div className="compact-subtitle-stack">
            {layout.visibleGroups.map((group, index) => (
              <CompactSubtitleRow key={group.key} group={group} current={index === layout.visibleGroups.length - 1}
                budget={index === layout.visibleGroups.length - 1 ? layout.latestBudget : undefined}
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
                {group.text && <p className="compact-original" lang={contentLanguageTag(group.language)}>{group.text}</p>}
                {group.translation && <p className="compact-translation" lang={contentLanguageTag(group.targetLanguage)}>{group.translation}{group.streaming && <span className="streaming-ellipsis">…</span>}</p>}
              </div>
            ))}
          </div>
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
      <button ref={buttonRef} type="button" className="compact-font-button" aria-label={label} title={label}
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

// Paint a wider white copy behind the original strokes only in transparent mode.
function CompactWindowIcon({ icon: Icon }: { icon: LucideIcon }) {
  return (
    <span className="compact-window-icon" aria-hidden="true">
      <Icon className="compact-icon-outline" size={15} strokeWidth={5.2} />
      <Icon className="compact-icon-foreground" size={15} />
    </span>
  );
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
