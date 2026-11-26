import { isTauri } from "@tauri-apps/api/core";

export const INTERFACE_SCALE_MIN = 75;
export const INTERFACE_SCALE_MAX = 150;
export const INTERFACE_SCALE_STEP = 5;
export const DEFAULT_INTERFACE_SCALE = 100;

const INTERFACE_SCALE_STORAGE_KEY = "vrcs.interfaceScalePercent";

export const INTERFACE_LAYOUT_CHANGE_EVENT = "vrcs:interface-layout-change";

let appliedZoom: number | null = null;
let zoomRequestId = 0;

export function normalizeInterfaceScale(value: unknown): number {
  if (value === null || value === undefined || value === "") return DEFAULT_INTERFACE_SCALE;
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed)) return DEFAULT_INTERFACE_SCALE;
  const stepped = Math.round(parsed / INTERFACE_SCALE_STEP) * INTERFACE_SCALE_STEP;
  return Math.min(INTERFACE_SCALE_MAX, Math.max(INTERFACE_SCALE_MIN, stepped));
}

export function readInterfaceScale(): number {
  try {
    return normalizeInterfaceScale(window.localStorage.getItem(INTERFACE_SCALE_STORAGE_KEY));
  } catch {
    return DEFAULT_INTERFACE_SCALE;
  }
}

export function writeInterfaceScale(value: number): void {
  try {
    window.localStorage.setItem(
      INTERFACE_SCALE_STORAGE_KEY,
      String(normalizeInterfaceScale(value)),
    );
  } catch {
    // Ignore unavailable storage; the in-memory scale remains active.
  }
}

export function interfaceScaleShortcutStep(event: Pick<
  KeyboardEvent,
  "altKey" | "code" | "ctrlKey" | "key" | "metaKey"
>): number {
  if (!event.ctrlKey || event.altKey || event.metaKey) return 0;
  if (event.code === "NumpadAdd" || event.key === "+" || event.key === "=") {
    return INTERFACE_SCALE_STEP;
  }
  if (event.code === "NumpadSubtract" || event.key === "-" || event.key === "_") {
    return -INTERFACE_SCALE_STEP;
  }
  return 0;
}

export function interfaceScaleFactor(value: unknown): number {
  return normalizeInterfaceScale(value) / 100;
}

export async function applyInterfaceScale(
  value: number,
  setZoom?: (scale: number) => Promise<void>,
): Promise<void> {
  const scale = interfaceScaleFactor(value);
  if (!setZoom && appliedZoom === scale) return;
  const requestId = ++zoomRequestId;
  if (setZoom) {
    await setZoom(scale);
  } else if (isTauri()) {
    try {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      if (requestId !== zoomRequestId) return;
      await getCurrentWebview().setZoom(scale);
    } catch (reason) {
      console.warn("Could not apply native webview zoom", reason);
      return;
    }
  }
  if (requestId !== zoomRequestId) return;
  appliedZoom = scale;
  if (typeof window !== "undefined") {
    window.dispatchEvent(new Event(INTERFACE_LAYOUT_CHANGE_EVENT));
  }
}
