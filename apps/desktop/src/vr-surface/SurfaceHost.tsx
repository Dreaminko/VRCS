import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { connectSurfaceBridge, type SurfaceBridge } from "./bridge";
import { VrSurface } from "./VrSurface";
import { markerBits, newerKey, sameKey, type Surface, type SurfaceActivity, type SurfaceKey, type SurfaceLayout, type SurfaceScrollState, type SurfaceState } from "./protocol";

export function SurfaceHost({ surface, connect = connectSurfaceBridge }: { surface: Surface; connect?: (surface: Surface, signal: AbortSignal) => Promise<SurfaceBridge> }) {
  const connection = useRef<SurfaceBridge | null>(null);
  const root = useRef<HTMLElement>(null);
  const [state, setState] = useState<SurfaceState | null>(null);
  const [measured, setMeasured] = useState<SurfaceKey | null>(null);
  const current = useRef<SurfaceKey | null>(null);
  const scrollKey = useRef<SurfaceKey | null>(null);
  const pendingScroll = useRef<{ viewport: HTMLElement; top: number } | null>(null);
  const scrollFrame = useRef<number | null>(null);
  const layoutRequest = useRef(0);
  const mounted = useRef(false);
  const [active, setActive] = useState(true);
  const activity = useRef<SurfaceActivity>({ active: true, request: 0 });
  const [paintSerial, setPaintSerial] = useState(1);
  const serial = useRef(1);
  const paintFrame = useRef<number | null>(null);
  const pendingState = useRef<SurfaceState | null>(null);
  const restored = useRef<SurfaceKey | null>(null);
  const cancelScroll = useCallback(() => {
    if (scrollFrame.current !== null) cancelAnimationFrame(scrollFrame.current);
    scrollFrame.current = null;
    pendingScroll.current = null;
  }, []);
  const markPaint = useCallback(() => {
    if (!mounted.current || !activity.current.active) return serial.current;
    serial.current = ((serial.current + 1) >>> 0) || 1;
    if (paintFrame.current === null) {
      const paint = () => {
        paintFrame.current = null;
        if (!mounted.current || !activity.current.active) return;
        const next = pendingState.current;
        pendingState.current = null;
        if (next) {
          setMeasured(previous => sameKey(previous, next.key) ? previous : null);
          setState(previous => {
            if (!previous || !sameKey(previous.key, next.key)) return next;
            const view = next.view.kind === "menu" && previous.view.kind === "menu" && next.view.hold_progress !== previous.view.hold_progress
              ? next.view : previous.view;
            return { ...next, key: previous.key, view };
          });
        }
        setPaintSerial(serial.current);
        if (root.current?.getAnimations({ subtree: true }).some(animation => animation.playState === "running" || animation.pending)) {
          paintFrame.current = requestAnimationFrame(() => {
            serial.current = ((serial.current + 1) >>> 0) || 1;
            paint();
          });
        }
      };
      paintFrame.current = requestAnimationFrame(paint);
    }
    return serial.current;
  }, []);

  const receiveScroll = useCallback((payload: SurfaceScrollState) => {
    if (!activity.current.active || !sameKey(scrollKey.current, payload.key) || !Number.isFinite(payload.delta)) return;
    const viewport = root.current?.querySelector<HTMLElement>("[data-vr-page-viewport]");
    if (!viewport) return;
    const before = pendingScroll.current?.viewport === viewport ? pendingScroll.current.top : viewport.scrollTop;
    // Clamp each input so reversing at an edge does not have to undo discarded overscroll.
    const top = Math.max(0, Math.min(viewport.scrollHeight - viewport.clientHeight, before + payload.delta * viewport.clientHeight));
    if (top === before) return;
    pendingScroll.current = { viewport, top };
    if (scrollFrame.current !== null) return;
    scrollFrame.current = requestAnimationFrame(() => {
      scrollFrame.current = null;
      const pending = pendingScroll.current;
      pendingScroll.current = null;
      if (!pending || !activity.current.active || !pending.viewport.isConnected) return;
      const { viewport, top } = pending;
      const before = viewport.scrollTop;
      viewport.scrollTop = top;
      if (viewport.scrollTop !== before) {
        // Update follow mode before incoming captions overtake the browser's scroll event.
        viewport.dispatchEvent(new Event("scroll"));
        markPaint();
      }
    });
  }, [markPaint]);

  useEffect(() => {
    let disposed = false;
    const abort = new AbortController();
    mounted.current = true;
    const receiveActivity = (next: SurfaceActivity) => {
      if (disposed || !Number.isSafeInteger(next.request) || next.request < activity.current.request) return;
      activity.current = next;
      setActive(next.active);
      if (!next.active) {
        cancelScroll();
        if (paintFrame.current !== null) cancelAnimationFrame(paintFrame.current);
        paintFrame.current = null;
        pendingState.current = null;
        const scrollTop = root.current?.querySelector<HTMLElement>("[data-vr-page-viewport]")?.scrollTop ?? null;
        void connection.current?.snapshot(current.current, next.request, scrollTop)
          .catch(error => console.error("VR surface snapshot failed", error));
      } else markPaint();
    };
    const receive = (next: SurfaceState | null, hoverUpdate = true) => {
      if (next?.activity && (!Number.isSafeInteger(next.activity.request) || next.activity.request < activity.current.request)) return;
      if (!disposed && next?.view.kind === surface && (newerKey(next.key, current.current) || (hoverUpdate && sameKey(current.current, next.key)))) {
        if (next.activity) receiveActivity(next.activity);
        const nextScrollKey = next.scroll_key ?? next.key;
        if (!sameKey(scrollKey.current, nextScrollKey)) cancelScroll();
        scrollKey.current = nextScrollKey;
        current.current = next.key;
        pendingState.current = next;
        markPaint();
      }
    };
    void (async () => {
      const bridge = await connect(surface, abort.signal);
      if (disposed) { bridge.dispose(); return; }
      connection.current = bridge;
      bridge.on("view", receive);
      bridge.on("activity", receiveActivity);
      bridge.on("repaint", () => {
        if (!disposed) markPaint();
      });
      if (surface === "reader" || surface === "dialogue" || surface === "player_audio" || surface === "dashboard") bridge.on("scroll", receiveScroll);
      receive(await bridge.ready(), false);
    })().catch((error) => { if (!disposed) console.error("VR surface connection failed", error); });
    return () => {
      disposed = true; mounted.current = false; layoutRequest.current++; cancelScroll();
      if (paintFrame.current !== null) cancelAnimationFrame(paintFrame.current);
      paintFrame.current = null; pendingState.current = null;
      abort.abort(); connection.current = null;
    };
  }, [surface, connect, markPaint, cancelScroll, receiveScroll]);

  const onLayout = useCallback((layout: SurfaceLayout) => {
    if (!activity.current.active) return;
    const request = ++layoutRequest.current;
    const paintSerial = markPaint();
    void connection.current?.layout(layout, paintSerial).then((accepted) => {
      if (mounted.current && request === layoutRequest.current && sameKey(current.current, layout.key)) setMeasured(accepted ? layout.key : null);
    }).catch((error) => { if (mounted.current) console.error("VR surface layout failed", error); });
  }, [surface, markPaint]);

  useLayoutEffect(() => {
    if (!state || sameKey(restored.current, state.key)) return;
    const viewport = root.current?.querySelector<HTMLElement>("[data-vr-page-viewport]");
    if (viewport && state.restore_scroll != null && Number.isFinite(state.restore_scroll)) {
      viewport.scrollTop = state.restore_scroll;
      markPaint();
    }
    restored.current = state.key;
  }, [state, markPaint]);

  useLayoutEffect(() => {
    if (!active || !activity.current.active || !state || !sameKey(measured, state.key) || paintSerial !== serial.current) return;
    // This is a request, not proof of a GPU paint. WGC verifies the same-frame marker.
    void connection.current?.paint(state.key, paintSerial)
      .catch(error => console.error("VR surface paint request failed", error));
  }, [active, state?.key, measured, paintSerial, surface]);

  useEffect(() => {
    const changed = () => { markPaint(); };
    document.fonts.addEventListener("loadingdone", changed);
    void document.fonts.ready.then(changed);
    return () => document.fonts.removeEventListener("loadingdone", changed);
  }, [markPaint]);

  useEffect(() => {
    if (!active || !state) return;
    if (root.current?.getAnimations({ subtree: true }).some(animation => animation.playState === "running" || animation.pending)) markPaint();
  }, [active, state, markPaint]);

  return <main ref={root} className="vr-surface-host" data-active={active}>
    {state && <VrSurface state={state} onLayout={onLayout} />}
    {state && sameKey(measured, state.key) && <div className="vr-surface-marker" aria-hidden="true">
      {markerBits(state.key, paintSerial).map((bit, index) => <i key={index} style={{ background: bit ? "#ffffff" : "#000000" }} />)}
    </div>}
  </main>;
}
