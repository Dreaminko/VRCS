import { useLayoutEffect, useRef, type CSSProperties, type ReactNode } from "react";
import { Captions, ChevronLeft, ChevronRight, Eye, GripHorizontal, Hand, Languages, MessageSquare, Mic, Pin, PinOff, Radio, ScanEye, Square, X } from "lucide-react";
import { VisionAnswerMarkdown } from "../vision/VisionAnswerMarkdown";
import { CaptionCards, Headset, OcrProgress } from "./PassiveSurfaces";
import { visibleCaptionMessages } from "./caption-visibility";
import { DashboardSurface, DashboardThumbnail } from "./DashboardSurface";
import { OcrResultSurface, OcrSelectionSurface } from "./OcrSurfaces";
import { menuStatus, pageGeometry, SURFACE_SIZE, type SurfaceLayout, type SurfaceState, type SurfaceView } from "./protocol";

import "./surface.css";

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
  const icons = [<MessageSquare />, <Captions />, <Radio />, view.controls.items[3]?.requested ? <Square /> : <Mic />];
  const progress = view.hold_progress;
  return <nav className="vr-menu-capsule">
    {view.controls.items.slice(0, 4).map((item, index) => <SurfaceButton state={state} id={index} key={index}
      {...menuStatus(item, view.controls.labels)}
      active={item.requested} primary={index === 3} disabled={!item.available || item.busy}>
      {icons[index]}
      {index === 0 && state.pressed === 0 && progress != null && <svg className="vr-menu-hold-progress" viewBox="0 0 160 160"
        role="progressbar" aria-label={item.label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progress * 100)}>
        <circle className="vr-menu-hold-track" cx="80" cy="80" r="72" />
        <circle cx="80" cy="80" r="72" pathLength={100} strokeDasharray={100} strokeDashoffset={100 * (1 - progress)} />
      </svg>}
    </SurfaceButton>)}
  </nav>;
}

function Dialogue({ state, view }: { state: SurfaceState; view: Extract<SurfaceView, { kind: "dialogue" }> }) {
  const labels = view.controls.labels;
  const messages = visibleCaptionMessages(view);
  return <>
    <nav className="vr-dialogue-toolbar vr-surface-capsule" data-vr-panel>
      <SurfaceButton state={state} id={0} label={labels.dragHint || "Point at the handle and hold grip to drag"} grip disabled={view.busy}>
        <GripHorizontal /><span>{view.busy ? labels.controlBusy || "Working" : labels.dialogueTitle || view.controls.items[0]?.label}</span>
      </SurfaceButton>
      <i className="vr-surface-divider" />
      <SurfaceButton state={state} id={1} label={labels.anchorWrist || "Wrist"} active={view.mode === "wrist"} disabled={view.busy}><Hand /></SurfaceButton>
      <SurfaceButton state={state} id={3} label={labels.followView || "Follow view"} active={view.mode === "world" && view.follow_view} disabled={view.busy || view.mode !== "world"}><Eye /></SurfaceButton>
      <SurfaceButton state={state} id={2} label={labels.anchorWorld || "World"} active={view.mode === "world"} disabled={view.busy}><Pin /></SurfaceButton>
    </nav>
    <section className="vr-dialogue-body vr-surface-panel" data-vr-panel>
      {view.notice && <p className="vr-caption-notice" role="status">{view.notice}</p>}
      {messages.length ? view.parallel
        ? <CaptionCards messages={messages} labels={view.labels} lines={messages.length <= 3 ? 2 : 1} fontSize={view.font_size} height={view.notice ? 440 : 550} />
        : <div className="vr-dialogue-messages">{messages.map((message, index) => <div className={`vr-dialogue-bubble ${message.side}`} key={index}>
          <div className="vr-caption-speaker">{message.speaker || (message.side === "right" ? view.labels?.me ?? "Me" : view.labels?.other ?? "Others")}</div>
          {message.text}
        </div>)}</div>
        : !view.notice && <p className="vr-surface-empty">{labels.emptyDialogue || "No recent dialogue"}</p>}
      {view.error && <p className="vr-surface-error" role="alert">{view.error}</p>}
    </section>
  </>;
}

function Reader({ state, view }: { state: SurfaceState; view: Extract<SurfaceView, { kind: "reader" }> }) {
  const viewport = useRef<HTMLDivElement>(null);
  const reader = view.reader;
  useLayoutEffect(() => {
    const element = viewport.current;
    if (element) element.scrollTop = pageGeometry(element.scrollHeight, element.clientHeight, reader.page, parseFloat(getComputedStyle(element).lineHeight) || 1).offset;
  }, [reader.mode, reader.block_index, reader.source, reader.translation_label, reader.source_only, reader.page]);
  const label = (key: string) => view.labels[key] || key;
  const vision = reader.mode === "vision";
  const busy = vision && ["listening", "finalizing", "requesting"].includes(reader.status);
  const error = vision && reader.status === "visionError";
  const translation = busy ? label(reader.status === "listening" ? "listeningHint" : "requestingHint")
    : error ? `${label("retryHint")}\n\n${reader.translation}` : reader.translation;
  return <section className="vr-reader-panel vr-surface-panel" data-vr-panel>
    <header className="vr-reader-header">
      <h1><ScanEye />{label(reader.title)}</h1>
      {!vision && reader.block_count > 0 && <div className="vr-reader-meta"><span>{label("block")} {reader.block_index + 1} / {reader.block_count}</span></div>}
    </header>
    <div className={`vr-reader-status${error ? " vr-surface-error" : ""}`}><span>{label(reader.status)}</span><span>{reader.detail}</span></div>
    <div ref={viewport} className="vr-reader-viewport" data-vr-page-viewport>
      <div className="vr-reader-content" data-vr-page-content>
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
      </>}
      {vision && reader.source && <SurfaceButton state={state} id={5} label={label(reader.source_only ? "backToAnswer" : "question")} active={reader.source_only}><MessageSquare /></SurfaceButton>}
      {(!vision || reader.source) && <i className="vr-surface-divider" />}
      <SurfaceButton state={state} id={6} label={label(reader.pinned ? "unpin" : "pin")} active={reader.pinned}>{reader.pinned ? <PinOff /> : <Pin />}</SurfaceButton>
      <SurfaceButton state={state} id={7} label={label(busy ? "cancel" : "close")}><X /></SurfaceButton>
    </nav>
  </section>;
}

export function VrSurface({ state, onLayout }: { state: SurfaceState; onLayout?: (layout: SurfaceLayout) => void }) {
  const root = useRef<HTMLDivElement>(null);
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
      if (disposed || element.querySelector("[data-vr-pending]")) return;
      const viewport = element.querySelector<HTMLElement>("[data-vr-page-viewport]");
      const content = element.querySelector<HTMLElement>("[data-vr-page-content]");
      const next = viewport && content && state.view.kind === "reader"
        ? pageGeometry(content.scrollHeight, viewport.clientHeight, state.view.reader.page, parseFloat(getComputedStyle(content).lineHeight) || fontSize * 1.65)
        : { count: 1, page: 0, offset: 0 };
      const bounds = element.getBoundingClientRect();
      if (!bounds.width || !bounds.height) return;
      const normalize = (rect: DOMRect): [number, number, number, number] => [(rect.left - bounds.left) / bounds.width, (rect.top - bounds.top) / bounds.height, (rect.right - bounds.left) / bounds.width, (rect.bottom - bounds.top) / bounds.height];
      const regions = [...element.querySelectorAll<HTMLButtonElement>("[data-vr-region]")].filter((button) => !button.disabled).map((button) => {
        return { id: Number(button.dataset.vrRegion), bounds: normalize(button.getBoundingClientRect()), ...(button.dataset.vrCircle ? { circle: true } : {}), ...(button.dataset.vrCommand ? { dashboard: JSON.parse(button.dataset.vrCommand) } : {}) };
      });
      const menu = element.querySelector(".vr-menu-capsule");
      const panels = [...element.querySelectorAll<HTMLElement>("[data-vr-panel]")].map(panel => {
        const rect = panel.getBoundingClientRect();
        const radius = Math.min(parseFloat(getComputedStyle(panel).borderTopLeftRadius) || 0, rect.width / 2, rect.height / 2);
        return { bounds: normalize(rect), radius: [radius / bounds.width, radius / bounds.height] as [number, number] };
      });
      const unplaced = element.querySelector<HTMLElement>("[data-vr-unplaced]")?.dataset.vrUnplaced;
      const layout: SurfaceLayout = { key: state.key, regions, ...(menu ? { menu_bounds: normalize(menu.getBoundingClientRect()) } : {}), ...(panels.length ? { panels } : {}), page_count: next.count, ...(unplaced ? { unplaced: JSON.parse(unplaced) as number[] } : {}) };
      const signature = JSON.stringify(layout);
      if (lastLayout.current !== signature) { lastLayout.current = signature; onLayout?.(layout); }
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    element.querySelectorAll("[data-vr-panel]").forEach(panel => observer.observe(panel));
    const content = element.querySelector("[data-vr-page-content]");
    if (content) observer.observe(content);
    void document.fonts.ready.then(measure);
    element.addEventListener("vr-surface-layout", measure);
    return () => { disposed = true; observer.disconnect(); element.removeEventListener("vr-surface-layout", measure); };
  }, [state.view, state.key.generation, state.key.revision, onLayout, fontSize]);

  const style = { width, height, "--vr-font-size": `${fontSize}px`, "--vr-background-opacity": opacity } as CSSProperties;
  return <div ref={root} className={`vr-surface vr-surface-${state.view.kind}`} data-theme={dark ? "dark" : "light"} style={style}>
    {state.view.kind === "menu" ? <Menu state={state} view={state.view} />
      : state.view.kind === "dialogue" ? <Dialogue state={state} view={state.view} />
        : state.view.kind === "reader" ? <Reader state={state} view={state.view} />
          : state.view.kind === "headset" ? <Headset view={state.view} />
            : state.view.kind === "ocr_progress" ? <OcrProgress view={state.view} />
              : state.view.kind === "dashboard" ? <DashboardSurface dashboard={state.view.dashboard} ui={state.view.ui} hovered={state.hovered} pressed={state.pressed} />
                : state.view.kind === "dashboard_thumbnail" ? <DashboardThumbnail />
                  : state.view.kind === "ocr_selection" ? <OcrSelectionSurface />
                    : state.view.kind === "ocr_result" ? <OcrResultSurface scene={state.view.scene} /> : null}
  </div>;
}
