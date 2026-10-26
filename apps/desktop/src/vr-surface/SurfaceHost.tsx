import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { VrSurface } from "./VrSurface";
import { markerBits, newerKey, sameKey, type Surface, type SurfaceActivity, type SurfaceKey, type SurfaceLayout, type SurfaceScrollState, type SurfaceState } from "./protocol";

export function SurfaceHost({ surface }: { surface: Surface }) {
  const root = useRef<HTMLElement>(null);
  const [state, setState] = useState<SurfaceState | null>(null);
  const [measured, setMeasured] = useState<SurfaceKey | null>(null);
  const current = useRef<SurfaceKey | null>(null);
  const layoutRequest = useRef(0);
  const mounted = useRef(false);
  const [active, setActive] = useState(true);
  const activity = useRef<SurfaceActivity>({ active: true, request: 0 });
  const [paintSerial, setPaintSerial] = useState(1);
  const serial = useRef(1);
  const restored = useRef<SurfaceKey | null>(null);
  const markPaint = useCallback(() => {
    if (!mounted.current || !activity.current.active) return serial.current;
    serial.current = ((serial.current + 1) >>> 0) || 1;
    setPaintSerial(serial.current);
    return serial.current;
  }, []);

  useEffect(() => {
    let disposed = false;
    const subscriptions: (() => void)[] = [];
    mounted.current = true;
    const receiveActivity = (next: SurfaceActivity) => {
      if (disposed || !Number.isSafeInteger(next.request) || next.request < activity.current.request) return;
      activity.current = next;
      setActive(next.active);
      if (!next.active) {
        const scrollTop = root.current?.querySelector<HTMLElement>("[data-vr-page-viewport]")?.scrollTop ?? null;
        void invoke("vr_surface_snapshot", { surface, key: current.current, activityRequest: next.request, scrollTop })
          .catch(error => console.error("VR surface snapshot failed", error));
      } else markPaint();
    };
    const receive = (next: SurfaceState | null, hoverUpdate = true) => {
      if (next?.activity && (!Number.isSafeInteger(next.activity.request) || next.activity.request < activity.current.request)) return;
      if (!disposed && next?.view.kind === surface && (newerKey(next.key, current.current) || (hoverUpdate && sameKey(current.current, next.key)))) {
        if (next.activity) receiveActivity(next.activity);
        if (!sameKey(current.current, next.key)) setMeasured(null);
        current.current = next.key;
        // Menu hold progress changes pixels under the same layout key.
        setState(previous => {
          if (!previous || !sameKey(previous.key, next.key)) return next;
          const view = next.view.kind === "menu" && previous.view.kind === "menu" && next.view.hold_progress !== previous.view.hold_progress
            ? next.view : previous.view;
          return { ...next, key: previous.key, view };
        });
        markPaint();
      }
    };
    void (async () => {
      const viewListener = await listen<SurfaceState>("vr-surface-view", (event) => receive(event.payload));
      const activityListener = await listen<SurfaceActivity>("vr-surface-activity", (event) => receiveActivity(event.payload), {
        target: { kind: "WebviewWindow", label: `vr-surface-${surface}` },
      });
      if (disposed) { viewListener(); activityListener(); return; }
      subscriptions.push(viewListener, activityListener);
      receive(await invoke<SurfaceState | null>("vr_surface_ready", { surface }), false);
    })().catch((error) => { if (!disposed) console.error("VR surface connection failed", error); });
    return () => { disposed = true; mounted.current = false; layoutRequest.current++; subscriptions.forEach(unlisten => unlisten()); };
  }, [surface, markPaint]);

  useEffect(() => {
    if (surface !== "reader" && surface !== "player_audio" && surface !== "dashboard") return;
    let disposed = false;
    const subscription = listen<SurfaceScrollState>("vr-surface-scroll", ({ payload }) => {
      if (disposed || !activity.current.active || !sameKey(current.current, payload.key) || !Number.isFinite(payload.delta)) return;
      const viewport = root.current?.querySelector<HTMLElement>("[data-vr-page-viewport]");
      if (viewport) {
        const before = viewport.scrollTop;
        viewport.scrollTop += payload.delta * viewport.clientHeight;
        if (viewport.scrollTop !== before) markPaint();
      }
    });
    void subscription.catch((error) => console.error("VR reader scroll connection failed", error));
    return () => { disposed = true; void subscription.then((unlisten) => unlisten()).catch(() => {}); };
  }, [surface, markPaint]);

  const onLayout = useCallback((layout: SurfaceLayout) => {
    if (!activity.current.active) return;
    const request = ++layoutRequest.current;
    const paintSerial = markPaint();
    void invoke<boolean>("vr_surface_layout", { surface, layout, paintSerial }).then((accepted) => {
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
    if (!active || !activity.current.active || !state || !sameKey(measured, state.key)) return;
    // This is a request, not proof of a GPU paint. WGC verifies the same-frame marker.
    void invoke("vr_surface_paint", { surface, key: state.key, paintSerial })
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
    let timer: ReturnType<typeof setTimeout>;
    const refreshAnimation = () => {
      if (!activity.current.active || !root.current?.getAnimations({ subtree: true }).some(animation => animation.playState === "running" || animation.pending)) return;
      markPaint();
      timer = setTimeout(refreshAnimation, 34);
    };
    refreshAnimation();
    return () => clearTimeout(timer);
  }, [active, state, markPaint]);

  return <main ref={root} className="vr-surface-host" data-active={active}>
    {state && <VrSurface state={state} onLayout={onLayout} />}
    {state && sameKey(measured, state.key) && <div className="vr-surface-marker" aria-hidden="true">
      {markerBits(state.key, paintSerial).map((bit, index) => <i key={index} style={{ background: bit ? "#ffffff" : "#000000" }} />)}
    </div>}
  </main>;
}
