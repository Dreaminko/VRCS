import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { ArrowLeft, ArrowRight, ChevronLeft, ChevronRight, GripHorizontal, Hand, Languages, MessageSquare, Mic, Monitor, Pin, PinOff, Radio, ScanEye, Square, X } from "lucide-react";
import { VisionAnswerMarkdown } from "../vision/VisionAnswerMarkdown";
import { Headset, OcrProgress } from "./PassiveSurfaces";
import { menuStatus, pageGeometry, SURFACE_SIZE, type SurfaceLayout, type SurfaceState, type SurfaceView } from "./protocol";

import "./surface.css";

type Paging = ReturnType<typeof pageGeometry>;

function SurfaceButton({ state, id, label, children, active = false, primary = false, disabled = false, grip = false, status }: {
  state: SurfaceState; id: number; label: string; children: ReactNode; active?: boolean; primary?: boolean; disabled?: boolean; grip?: boolean;
  status?: string;
}) {
  const hovered = state.hovered === id;
  return <button type="button" tabIndex={-1} disabled={disabled} aria-label={label} aria-pressed={active} aria-busy={status === "busy" || undefined} data-status={status}
    data-vr-region={id} data-vr-circle={!grip || undefined}
    className={`vr-surface-button${grip ? " vr-surface-grip" : ""}${active ? " active" : ""}${primary ? " primary" : ""}${hovered ? " hovered" : ""}${state.pressed === id ? " pressed" : ""}`}>
    {children}
    {hovered && label && <span className="vr-surface-tooltip">{label}</span>}
  </button>;
}

function Menu({ state, view }: { state: SurfaceState; view: Extract<SurfaceView, { kind: "menu" }> }) {
  const icons = [<MessageSquare />, <Monitor />, <Radio />, view.controls.items[3]?.requested ? <Square /> : <Mic />];
  return <nav className="vr-menu-capsule">
    {view.controls.items.slice(0, 4).map((item, index) => <SurfaceButton state={state} id={index} key={index}
      {...menuStatus(item, view.controls.labels)}
      active={item.requested} primary={index === 3} disabled={!item.available || item.busy}>{icons[index]}</SurfaceButton>)}
  </nav>;
}

function Dialogue({ state, view }: { state: SurfaceState; view: Extract<SurfaceView, { kind: "dialogue" }> }) {
  const labels = view.controls.labels;
  return <>
    <nav className="vr-dialogue-toolbar vr-surface-capsule">
      <SurfaceButton state={state} id={0} label={labels.dragHint || "Point at the handle and hold grip to drag"} grip disabled={view.busy}>
        <GripHorizontal /><span>{view.busy ? labels.controlBusy || "Working" : labels.dialogueTitle || view.controls.items[0]?.label}</span>
      </SurfaceButton>
      <i className="vr-surface-divider" />
      <SurfaceButton state={state} id={1} label={labels.anchorWrist || "Wrist"} active={view.mode === "wrist"} disabled={view.busy}><Hand /></SurfaceButton>
      <SurfaceButton state={state} id={2} label={labels.anchorWorld || "World"} active={view.mode === "world"} disabled={view.busy}><Pin /></SurfaceButton>
    </nav>
    <section className="vr-dialogue-body vr-surface-panel">
      {view.messages.length ? <div className="vr-dialogue-messages">{view.messages.map((message, index) => <p className={`vr-dialogue-bubble ${message.side}`} key={index}>{message.text}</p>)}</div>
        : <p className="vr-surface-empty">{labels.emptyDialogue || "No recent dialogue"}</p>}
      {view.error && <p className="vr-surface-error" role="alert">{view.error}</p>}
    </section>
  </>;
}

function Reader({ state, view, paging }: { state: SurfaceState; view: Extract<SurfaceView, { kind: "reader" }>; paging: Paging }) {
  const reader = view.reader;
  const label = (key: string) => view.labels[key] || key;
  const vision = reader.mode === "vision";
  const busy = vision && ["listening", "finalizing", "requesting"].includes(reader.status);
  const error = vision && reader.status === "visionError";
  const translation = busy ? label(reader.status === "listening" ? "listeningHint" : "requestingHint")
    : error ? `${label("retryHint")}\n\n${reader.translation}` : reader.translation;
  return <section className="vr-reader-panel vr-surface-panel">
    <header className="vr-reader-header">
      <h1><ScanEye />{label(reader.title)}</h1>
      <div className="vr-reader-meta"><span>{!vision && reader.block_count > 0 ? `${label("block")} ${reader.block_index + 1} / ${reader.block_count} · ` : ""}{label("page")} {paging.page + 1} / {paging.count}</span></div>
    </header>
    <div className={`vr-reader-status${error ? " vr-surface-error" : ""}`}><span>{label(reader.status)}</span><span>{reader.detail}</span></div>
    <div className="vr-reader-viewport" data-vr-page-viewport>
      <div className="vr-reader-content" data-vr-page-content style={{ transform: `translateY(-${paging.offset}px)` }}>
        {reader.source && <section className="vr-reader-card">
          <h2>{label(vision ? "question" : "source")}</h2><p className="vr-reader-plain">{reader.source}</p>
        </section>}
        {!reader.source_only && translation && <section className={`vr-reader-card${error ? " error" : ""}`}>
          <h2>{label(error ? "visionError" : reader.translation_label || "answer")}</h2>
          {vision && reader.status === "complete" ? <VisionAnswerMarkdown vr>{translation}</VisionAnswerMarkdown> : <p className="vr-reader-plain">{translation}</p>}
        </section>}
      </div>
    </div>
    <nav className="vr-reader-toolbar vr-surface-capsule">
      {!vision && <>
        <SurfaceButton state={state} id={0} label={label("previousBlock")} disabled={reader.block_index <= 0}><ChevronLeft /></SurfaceButton>
        <SurfaceButton state={state} id={1} label={label("nextBlock")} disabled={reader.block_index + 1 >= reader.block_count}><ChevronRight /></SurfaceButton>
        {reader.language_count > 1 && <SurfaceButton state={state} id={4} label={label("language")}><Languages /></SurfaceButton>}
        <i className="vr-surface-divider" />
      </>}
      <SurfaceButton state={state} id={2} label={label("previousPage")} disabled={paging.page <= 0}><ArrowLeft /></SurfaceButton>
      <SurfaceButton state={state} id={3} label={label("nextPage")} disabled={paging.page + 1 >= paging.count}><ArrowRight /></SurfaceButton>
      {vision && reader.source && <SurfaceButton state={state} id={5} label={label(reader.source_only ? "backToAnswer" : "question")} active={reader.source_only}><MessageSquare /></SurfaceButton>}
      <i className="vr-surface-divider" />
      <SurfaceButton state={state} id={6} label={label(reader.pinned ? "unpin" : "pin")} active={reader.pinned}>{reader.pinned ? <PinOff /> : <Pin />}</SurfaceButton>
      <SurfaceButton state={state} id={7} label={label(busy ? "cancel" : "close")}><X /></SurfaceButton>
    </nav>
  </section>;
}

export function VrSurface({ state, onLayout }: { state: SurfaceState; onLayout?: (layout: SurfaceLayout) => void }) {
  const root = useRef<HTMLDivElement>(null);
  const [paging, setPaging] = useState<Paging>({ count: 1, page: 0, offset: 0 });
  const lastLayout = useRef("");
  const [width, height] = SURFACE_SIZE[state.view.kind];
  const dark = "dark" in state.view ? state.view.dark : state.view.controls.dark;
  const fontSize = "font_size" in state.view ? Math.min(state.view.kind === "headset" ? 68 : 64, Math.max(16, state.view.font_size)) : 28;
  const opacity = "opacity" in state.view ? Math.min(1, Math.max(0, state.view.opacity)) : 1;

  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    let disposed = false;
    const measure = () => {
      if (disposed) return;
      const viewport = element.querySelector<HTMLElement>("[data-vr-page-viewport]");
      const content = element.querySelector<HTMLElement>("[data-vr-page-content]");
      const next = viewport && content && state.view.kind === "reader"
        ? pageGeometry(content.scrollHeight, viewport.clientHeight, state.view.reader.page, parseFloat(getComputedStyle(content).lineHeight) || fontSize * 1.65)
        : { count: 1, page: 0, offset: 0 };
      setPaging((current) => current.count === next.count && current.page === next.page && current.offset === next.offset ? current : next);
      const bounds = element.getBoundingClientRect();
      if (!bounds.width || !bounds.height) return;
      const regions = [...element.querySelectorAll<HTMLButtonElement>("[data-vr-region]")].filter((button) => !button.disabled).map((button) => {
        const rect = button.getBoundingClientRect();
        return { id: Number(button.dataset.vrRegion), bounds: [(rect.left - bounds.left) / bounds.width, (rect.top - bounds.top) / bounds.height, (rect.right - bounds.left) / bounds.width, (rect.bottom - bounds.top) / bounds.height] as [number, number, number, number], ...(button.dataset.vrCircle ? { circle: true } : {}) };
      });
      const layout = { key: state.key, regions, page_count: next.count };
      const signature = JSON.stringify(layout);
      if (lastLayout.current !== signature) { lastLayout.current = signature; onLayout?.(layout); }
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    const content = element.querySelector("[data-vr-page-content]");
    if (content) observer.observe(content);
    void document.fonts.ready.then(measure);
    return () => { disposed = true; observer.disconnect(); };
  }, [state, onLayout, fontSize, paging.count, paging.page]);

  const style = { width, height, "--vr-font-size": `${fontSize}px`, "--vr-background-opacity": opacity } as CSSProperties;
  return <div ref={root} className={`vr-surface vr-surface-${state.view.kind}`} data-theme={dark ? "dark" : "light"} style={style}>
    {state.view.kind === "menu" ? <Menu state={state} view={state.view} />
      : state.view.kind === "dialogue" ? <Dialogue state={state} view={state.view} />
        : state.view.kind === "reader" ? <Reader state={state} view={state.view} paging={paging} />
          : state.view.kind === "headset" ? <Headset view={state.view} /> : <OcrProgress view={state.view} />}
  </div>;
}
