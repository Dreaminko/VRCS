import type { Surface, SurfaceActivity, SurfaceKey, SurfaceLayout, SurfacePointerState, SurfaceScrollState, SurfaceState } from "./protocol";

export interface WebviewMessagePort {
  postMessage(message: Record<string, unknown>): void;
  addEventListener(event: "message", listener: (event: { data: unknown }) => void): void;
  removeEventListener(event: "message", listener: (event: { data: unknown }) => void): void;
}

export interface SurfaceEvents {
  view: SurfaceState;
  activity: SurfaceActivity;
  pointer: SurfacePointerState;
  scroll: SurfaceScrollState;
  repaint: null;
}

export interface SurfaceBridge {
  ready(): Promise<SurfaceState | null>;
  layout(layout: SurfaceLayout, paintSerial: number): Promise<boolean>;
  paint(key: SurfaceKey, paintSerial: number): Promise<boolean>;
  snapshot(key: SurfaceKey | null, activityRequest: number, scrollTop: number | null): Promise<boolean>;
  fault(stage: "startup" | "render" | "unhandled"): Promise<boolean>;
  on<K extends keyof SurfaceEvents>(event: K, listener: (payload: SurfaceEvents[K]) => void): () => void;
  dispose(): void;
}

let nextRequestId = 1;

export function connectSurfaceBridge(surface: Surface, signal: AbortSignal): Promise<SurfaceBridge> {
  const port = (window as Window & { chrome?: { webview?: WebviewMessagePort } }).chrome?.webview;
  if (!port) return Promise.reject(new Error("Native surface bridge is unavailable"));
  return new Promise((resolve, reject) => {
    let nonce: string | null = null;
    let disposed = false;
    const listeners = new Map<keyof SurfaceEvents, Set<(payload: never) => void>>();
    const pending = new Map<number, {
      resolve: (value: unknown) => void;
      reject: (reason: Error) => void;
      timer: ReturnType<typeof setTimeout>;
    }>();
    const handshakeTimer = setTimeout(() => close(new Error("Surface handshake timed out")), 20000);
    function close(error = new Error("Surface bridge closed")) {
      if (disposed) return;
      disposed = true;
      clearTimeout(handshakeTimer);
      signal.removeEventListener("abort", abort);
      window.removeEventListener("pagehide", abort);
      port!.removeEventListener("message", receive);
      for (const request of pending.values()) {
        clearTimeout(request.timer);
        request.reject(error);
      }
      pending.clear();
      listeners.clear();
      reject(error);
    }
    function abort() { close(); }
    function request<T>(op: string, payload: Record<string, unknown>): Promise<T> {
      if (disposed || !nonce) return Promise.reject(new Error("Surface bridge closed"));
      if (pending.size >= 64) return Promise.reject(new Error("Too many pending surface requests"));
      if (!Number.isSafeInteger(nextRequestId)) return Promise.reject(new Error("Surface request IDs exhausted"));
      const requestId = nextRequestId++;
      return new Promise<T>((resolveRequest, rejectRequest) => {
        const timer = setTimeout(() => {
          pending.delete(requestId);
          rejectRequest(new Error("Surface request timed out"));
        }, 5000);
        pending.set(requestId, { resolve: value => resolveRequest(value as T), reject: rejectRequest, timer });
        try {
          port!.postMessage({ version: 1, nonce, request_id: requestId, op, payload });
        } catch {
          clearTimeout(timer);
          pending.delete(requestId);
          rejectRequest(new Error("Surface message delivery failed"));
        }
      });
    }
    const bridge: SurfaceBridge = {
      ready: () => request("ready", {}),
      layout: (layout, paintSerial) => request("layout", { layout, paint_serial: paintSerial }),
      paint: (key, paintSerial) => request("paint", { key, paint_serial: paintSerial }),
      snapshot: (key, activityRequest, scrollTop) => request("snapshot", { key, activity_request: activityRequest, scroll_top: scrollTop }),
      fault: stage => request("fault", { stage }),
      on(event, listener) {
        if (disposed) return () => {};
        let group = listeners.get(event);
        if (!group) { group = new Set(); listeners.set(event, group); }
        const callback = listener as (payload: never) => void;
        group.add(callback);
        return () => { group.delete(callback); };
      },
      dispose: close,
    };
    function receive({ data }: { data: unknown }) {
      if (disposed || !data || typeof data !== "object") return;
      const message = data as Record<string, unknown>;
      if (message.version !== 1) return;
      if (message.op === "welcome" && !nonce) {
        const host = message.host as Record<string, unknown> | undefined;
        if (host?.surface !== surface || !Number.isInteger(host.host_epoch) || Number(host.host_epoch) < 1
          || typeof message.nonce !== "string" || !message.nonce || message.nonce.length > 128) {
          close(new Error("Invalid surface identity"));
          return;
        }
        nonce = message.nonce;
        clearTimeout(handshakeTimer);
        resolve(bridge);
      } else if (nonce && message.nonce === nonce) {
        if (message.op === "reply" && typeof message.request_id === "number" && typeof message.ok === "boolean") {
          const waiter = pending.get(message.request_id);
          if (!waiter) return;
          pending.delete(message.request_id);
          clearTimeout(waiter.timer);
          if (message.ok) waiter.resolve(message.value);
          else waiter.reject(new Error(typeof message.error === "string" ? message.error : "Surface request rejected"));
        } else if (message.op === "event") {
          for (const listener of listeners.get(message.event as keyof SurfaceEvents) ?? []) {
            listener(message.payload as never);
          }
        }
      }
    }
    port.addEventListener("message", receive);
    signal.addEventListener("abort", abort, { once: true });
    window.addEventListener("pagehide", abort, { once: true });
    if (signal.aborted) { close(); return; }
    try { port.postMessage({ version: 1, op: "connect" }); }
    catch { close(new Error("Surface connection failed")); }
  });
}
