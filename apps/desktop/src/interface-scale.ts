import { isTauri } from "@tauri-apps/api/core";

export const INTERFACE_SCALE_MIN = 75;
export const INTERFACE_SCALE_MAX = 150;
export const INTERFACE_SCALE_STEP = 5;
export const DEFAULT_INTERFACE_SCALE = 100;

const INTERFACE_SCALE_STORAGE_KEY = "vrcs.interfaceScalePercent";

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
    // Keep the setting usable for this session when storage is unavailable.
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

export async function applyInterfaceScale(value: number): Promise<void> {
  const normalized = normalizeInterfaceScale(value);
  const scaleFactor = normalized / 100;

  if (isTauri()) {
    document.documentElement.style.removeProperty("zoom");
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    await getCurrentWebview().setZoom(scaleFactor);
    return;
  }

  document.documentElement.style.setProperty("zoom", String(scaleFactor));
}
