import { useLayoutEffect, useRef } from "react";
import { Check, Circle, LoaderCircle, Minus, ScanEye, X } from "lucide-react";
import type { SurfaceView } from "./protocol";
import { visibleCaption, visibleTail } from "./headset-layout";

function Caption({ text, lines, speaker, fontSize }: { text: string; lines: number; speaker: boolean; fontSize: number }) {
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const slot = root.current;
    const content = slot?.firstElementChild as HTMLSpanElement | null;
    if (!slot || !content) return;
    let disposed = false;
    const measure = () => {
      if (disposed) return;
      const fits = (candidate: string) => {
        content.textContent = candidate;
        return content.scrollWidth <= slot.clientWidth && content.scrollHeight <= slot.clientHeight;
      };
      content.textContent = (speaker ? visibleCaption : visibleTail)(text, fits);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(slot);
    void document.fonts.ready.then(measure);
    return () => { disposed = true; observer.disconnect(); };
  }, [text, lines, speaker, fontSize]);
  return <div ref={root} className="vr-headset-language" style={{ height: 82 * lines }}>
    <span style={{ whiteSpace: lines === 1 ? "nowrap" : "pre-wrap" }}>{text}</span>
  </div>;
}

export function Headset({ view }: { view: Extract<SurfaceView, { kind: "headset" }> }) {
  if (!view.text) return null;
  const lines = Math.min(4, Math.max(1, view.lines_per_language));
  return <section className="vr-headset-panel vr-surface-panel" aria-live="polite">
    {view.text.split("\n", 4).map((text, index) => <Caption key={index} text={text} lines={lines} speaker={index === 0} fontSize={view.font_size} />)}
  </section>;
}

const stageIcons = { pending: Circle, active: LoaderCircle, complete: Check, skipped: Minus, failed: X };

export function OcrProgress({ view }: { view: Extract<SurfaceView, { kind: "ocr_progress" }> }) {
  const progress = view.progress;
  if (!progress) return null;
  const failed = progress.stages.includes("failed");
  const active = progress.stages.includes("active");
  const StatusIcon = failed ? X : active ? LoaderCircle : ScanEye;
  return <section className="vr-progress-panel vr-surface-panel" role="status">
    <header className={failed ? "vr-surface-error" : undefined}>
      <StatusIcon className={active && !failed ? "vr-progress-spinner" : undefined} /><strong>{progress.status}</strong>
    </header>
    <ol className="vr-progress-stages">
      {progress.stages.map((stage, index) => {
        const Icon = stageIcons[stage];
        const fraction = index === 2 ? progress.translation_fraction : null;
        return <li key={index} data-stage={stage}>
          <span className="vr-progress-stage-label"><Icon className={stage === "active" ? "vr-progress-spinner" : undefined} />{progress.stage_labels[index]}</span>
          <div className={`vr-progress-track${stage === "active" && (!fraction || fraction[1] <= 0) ? " indeterminate" : ""}`} role="progressbar" aria-label={progress.stage_labels[index]} aria-valuemin={0} aria-valuemax={1}
            aria-valuenow={stage === "complete" ? 1 : fraction && fraction[1] > 0 ? Math.min(1, fraction[0] / fraction[1]) : undefined}>
            <i style={{ width: stage === "complete" ? "100%" : fraction && fraction[1] > 0 ? `${Math.min(1, fraction[0] / fraction[1]) * 100}%` : undefined }} />
          </div>
        </li>;
      })}
    </ol>
    <p>{progress.detail}</p>
  </section>;
}
