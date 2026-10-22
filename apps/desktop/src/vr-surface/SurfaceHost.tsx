import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { VrSurface } from "./VrSurface";
import { SurfacePointer } from "./SurfacePointer";
import { hasPointer, markerBits, newerKey, sameKey, type Surface, type SurfaceKey, type SurfaceLayout, type SurfacePointerState, type SurfaceState } from "./protocol";

function LivePointer({ state }: { state: SurfaceState | null }) {
  const [pointer, setPointer] = useState<SurfacePointerState | null>(null);
  useEffect(() => {
    let disposed = false;
    const subscription = listen<SurfacePointerState>("vr-surface-pointer", ({ payload }) => {
      if (!disposed) setPointer((current) => newerKey(payload.key, current?.key ?? null) || sameKey(current?.key ?? null, payload.key) ? payload : current);
    });
    void subscription.catch((error) => console.error("VR pointer connection failed", error));
    return () => { disposed = true; void subscription.then((unlisten) => unlisten()).catch(() => {}); };
  }, []);
  return state && <SurfacePointer point={pointer && sameKey(pointer.key, state.key) ? pointer.point : state.pointer} />;
}

export function SurfaceHost({ surface }: { surface: Surface }) {
  const [state, setState] = useState<SurfaceState | null>(null);
  const [measured, setMeasured] = useState<SurfaceKey | null>(null);
  const current = useRef<SurfaceKey | null>(null);
  const layoutRequest = useRef(0);
  const mounted = useRef(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    mounted.current = true;
    const receive = (next: SurfaceState | null, hoverUpdate = true) => {
      if (!disposed && next?.view.kind === surface && (newerKey(next.key, current.current) || (hoverUpdate && sameKey(current.current, next.key)))) {
        if (!sameKey(current.current, next.key)) setMeasured(null);
        current.current = next.key;
        setState(next);
      }
    };
    void (async () => {
      unlisten = await listen<SurfaceState>("vr-surface-view", (event) => receive(event.payload));
      if (disposed) { unlisten(); return; }
      receive(await invoke<SurfaceState | null>("vr_surface_ready", { surface }), false);
    })().catch((error) => { if (!disposed) console.error("VR surface connection failed", error); });
    return () => { disposed = true; mounted.current = false; layoutRequest.current++; unlisten?.(); };
  }, [surface]);

  const onLayout = useCallback((layout: SurfaceLayout) => {
    const request = ++layoutRequest.current;
    void invoke<boolean>("vr_surface_layout", { surface, layout }).then((accepted) => {
      if (mounted.current && request === layoutRequest.current && sameKey(current.current, layout.key)) setMeasured(accepted ? layout.key : null);
    }).catch((error) => { if (mounted.current) console.error("VR surface layout failed", error); });
  }, [surface]);

  return <main className="vr-surface-host">
    {state && <VrSurface state={state} onLayout={onLayout} />}
    {hasPointer(surface) && <LivePointer state={state} />}
    {state && sameKey(measured, state.key) && <div className="vr-surface-marker" aria-hidden="true">
      {markerBits(state.key).map((bit, index) => <i key={index} style={{ background: bit ? "#ffffff" : "#000000" }} />)}
    </div>}
  </main>;
}
