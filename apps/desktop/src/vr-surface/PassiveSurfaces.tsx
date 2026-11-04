import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { AudioLines, Camera, Check, Gamepad2, KeyRound, Languages, MessageSquare, Minus, Monitor, ScanText, Shield, X } from "lucide-react";
import type { CaptionMessage, CaptionLabels, SurfaceView } from "./protocol";
import { visibleTail } from "./headset-layout";
import { visibleCaptionMessages } from "./caption-visibility";

const CAPTION_LINE_HEIGHT = 1.3;

function Caption({ text, lines, fontSize }: { text: string; lines: number; fontSize: number }) {
  const root = useRef<HTMLDivElement>(null);
  const value = useRef({ text, lines, fontSize });
  const measure = useRef(() => {});
  useLayoutEffect(() => {
    const slot = root.current;
    const content = slot?.firstElementChild as HTMLSpanElement | null;
    if (!slot || !content) return;
    let previous: (typeof value.current & { width: number; height: number }) | null = null;
    measure.current = () => {
      const next = value.current;
      const width = slot.clientWidth;
      if (!width) return;
      const height = Math.ceil(parseFloat(getComputedStyle(content).lineHeight) * next.lines);
      if (previous && previous.text === next.text && previous.lines === next.lines
        && previous.fontSize === next.fontSize
        && previous.width === width && previous.height === height) return;
      previous = { ...next, width, height };
      slot.style.maxHeight = `${height}px`;
      const fits = (candidate: string) => {
        content.textContent = candidate;
        return content.scrollWidth <= width + 1 && content.scrollHeight <= height + 1;
      };
      content.textContent = visibleTail(next.text, fits);
    };
    const observer = new ResizeObserver(() => measure.current());
    observer.observe(slot);
    const fontsChanged = () => { previous = null; measure.current(); };
    document.fonts.addEventListener("loadingdone", fontsChanged);
    return () => {
      observer.disconnect();
      document.fonts.removeEventListener("loadingdone", fontsChanged);
      measure.current = () => {};
    };
  }, []);
  useLayoutEffect(() => {
    value.current = { text, lines, fontSize };
    measure.current();
  }, [text, lines, fontSize]);
  return <div ref={root} className="vr-headset-language" style={{ fontSize }}>
    <span style={{ lineHeight: CAPTION_LINE_HEIGHT, whiteSpace: lines === 1 ? "nowrap" : "pre-wrap" }}>{text}</span>
  </div>;
}

function HeadsetCaptions({ messages, labels, lines, fontSize, height }: {
  messages: CaptionMessage[]; labels?: CaptionLabels; lines: number; fontSize: number; height: number;
}) {
  const rows = messages.length;
  const languageCount = messages.reduce((count, message) => count + message.text.split("\n", 4).length, 0);
  const requiredHeight = fontSize * (rows * 0.75 * 1.1 + languageCount * lines * CAPTION_LINE_HEIGHT)
    + rows * 14 + (languageCount - rows) * 8 + Math.max(0, rows - 1) * 12;
  const scale = height > 0 ? Math.max(0.01, Math.min(1, (height - languageCount) / Math.max(1, requiredHeight))) : 1;
  const size = fontSize * scale;
  return <div className="vr-caption-grid" style={{ gridTemplateColumns: "minmax(0, 1fr)", gap: 12 * scale,
    "--vr-caption-language-gap": `${8 * scale}px` } as CSSProperties}>
    {messages.map((message, index) => <article key={index} className={`vr-caption-card ${message.side}`} style={{ padding: `${4 * scale}px 8px` }}>
      <div className="vr-caption-speaker" style={{ fontSize: size * 0.75, marginBottom: 6 * scale }} title={message.speaker || undefined}>
        {message.original?.source === "vrchat_chatbox" && <MessageSquare size="1em" aria-label="Chatbox" />}
        {message.speaker || (message.side === "right" ? labels?.me ?? "Me" : labels?.other ?? "Others")}
      </div>
      {message.text.split("\n", 4).map((text, language) => <Caption key={language} text={text} lines={lines} fontSize={size} />)}
    </article>)}
  </div>;
}

export function Headset({ view }: { view: Extract<SurfaceView, { kind: "headset" }> }) {
  const root = useRef<HTMLElement>(null);
  const [height, setHeight] = useState(0);
  const messages = visibleCaptionMessages(view);
  const visible = messages.length > 0 || !!view.notice;
  useLayoutEffect(() => {
    const panel = root.current;
    const surface = panel?.parentElement;
    if (!panel || !surface) return;
    const measure = () => {
      const outer = getComputedStyle(surface);
      const inner = getComputedStyle(panel);
      const notice = panel.querySelector<HTMLElement>(".vr-caption-notice");
      const noticeStyle = notice && getComputedStyle(notice);
      setHeight(surface.clientHeight - parseFloat(outer.paddingTop) - parseFloat(outer.paddingBottom)
        - parseFloat(inner.paddingTop) - parseFloat(inner.paddingBottom)
        - parseFloat(inner.borderTopWidth) - parseFloat(inner.borderBottomWidth)
        - (notice && noticeStyle ? notice.offsetHeight + parseFloat(noticeStyle.marginTop) + parseFloat(noticeStyle.marginBottom) : 0));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(surface);
    observer.observe(panel);
    return () => observer.disconnect();
  }, [visible, view.notice]);
  if (!visible) return null;
  const lines = Math.min(4, Math.max(1, view.lines_per_language));
  return <section ref={root} className="vr-headset-panel vr-surface-panel" aria-live="polite">
    {view.notice && <p className="vr-caption-notice" role="status">{view.notice}</p>}
    <HeadsetCaptions messages={messages} labels={view.labels} lines={lines} fontSize={Math.min(68, Math.max(16, view.font_size))} height={height} />
  </section>;
}

const progressIcons = {
  ocr: [Camera, ScanText, Languages, Monitor],
  key: [Shield, Gamepad2, KeyRound, AudioLines],
};
const stageIcons = { complete: Check, skipped: Minus, failed: X };

export function OcrProgress({ view }: { view: Extract<SurfaceView, { kind: "ocr_progress" }> }) {
  const progress = view.progress;
  const fraction = progress?.translation_fraction;
  const value = fraction && fraction[1] > 0 ? Math.min(1, fraction[0] / fraction[1]) : null;
  const previous = useRef<{ scanId?: number; value: number | null }>({ value: null });
  const smooth = value !== null && previous.current.value !== null
    && previous.current.scanId === progress?.scan_id && value >= previous.current.value;
  useLayoutEffect(() => { previous.current = { scanId: progress?.scan_id, value }; }, [progress?.scan_id, value]);
  if (!progress) return null;
  const icons = progressIcons[progress.task ?? "ocr"];
  const failed = progress.stages.includes("failed");
  const complete = progress.stages.every(stage => stage === "complete");
  const StatusIcon = failed ? X : complete ? Check : progress.task === "key" ? KeyRound : ScanText;
  return <section className="vr-progress-panel vr-surface-panel" role="status">
    <header className={failed ? "vr-surface-error" : undefined}>
      <StatusIcon /><strong>{progress.status}</strong>
    </header>
    <ol className="vr-progress-stages">
      {progress.stages.map((stage, index) => {
        const Icon = stage === "pending" || stage === "active" ? icons[index] ?? ScanText : stageIcons[stage];
        const fraction = index === 2 ? progress.translation_fraction : null;
        return <li key={index} data-stage={stage}>
          <span className="vr-progress-stage-label"><Icon />{progress.stage_labels[index]}</span>
          <div className={`vr-progress-track${stage === "active" && (!fraction || fraction[1] <= 0) ? " indeterminate" : ""}`} role="progressbar" aria-label={progress.stage_labels[index]} aria-valuemin={0} aria-valuemax={1}
            aria-valuenow={stage === "complete" ? 1 : fraction && fraction[1] > 0 ? Math.min(1, fraction[0] / fraction[1]) : undefined}>
            <i className={index === 2 && smooth ? "smooth" : undefined} style={{ width: stage === "complete" ? "100%" : fraction && fraction[1] > 0 ? `${Math.min(1, fraction[0] / fraction[1]) * 100}%` : undefined }} />
          </div>
        </li>;
      })}
    </ol>
    <p>{progress.detail}</p>
  </section>;
}
