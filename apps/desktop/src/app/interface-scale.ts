export const INTERFACE_SCALE_MIN = 75;
export const INTERFACE_SCALE_MAX = 150;
export const INTERFACE_SCALE_STEP = 5;
export const DEFAULT_INTERFACE_SCALE = 100;

const INTERFACE_SCALE_STORAGE_KEY = "vrcs.interfaceScalePercent";
const INTERFACE_SCALE_PROPERTY = "--interface-scale";
const INTERFACE_SCALE_INVERSE_PROPERTY = "--interface-scale-inverse";
const INTERFACE_LAYOUT_WIDTH_PROPERTY = "--interface-layout-width";
const INTERFACE_LAYOUT_HEIGHT_PROPERTY = "--interface-layout-height";
const INTERFACE_OVERLAY_GUTTER_PROPERTY = "--interface-overlay-gutter";

export const INTERFACE_LAYOUT_CHANGE_EVENT = "vrcs:interface-layout-change";
export const COMPACT_OVERLAY_HEIGHT = 720;

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

export function interfaceScaleFactors(value: unknown): {
  scale: number;
  inverse: number;
} {
  const scale = normalizeInterfaceScale(value) / 100;
  return { scale, inverse: 1 / scale };
}

export function readAppliedInterfaceScaleFactor(): number {
  if (typeof document === "undefined") return 1;
  const value = Number(
    document.documentElement.style.getPropertyValue(INTERFACE_SCALE_PROPERTY),
  );
  return Number.isFinite(value) && value > 0 ? value : 1;
}

export function interfaceLayoutPixels(
  value: number,
  scale = readAppliedInterfaceScaleFactor(),
): number {
  return value / (Number.isFinite(scale) && scale > 0 ? scale : 1);
}

export function interfaceViewportMetrics(
  width: number,
  height: number,
  scale = readAppliedInterfaceScaleFactor(),
): { width: number; height: number; overlayGutter: number } {
  const layoutWidth = interfaceLayoutPixels(width, scale);
  const layoutHeight = interfaceLayoutPixels(height, scale);
  return {
    width: layoutWidth,
    height: layoutHeight,
    overlayGutter: layoutHeight < COMPACT_OVERLAY_HEIGHT ? 12 : 24,
  };
}

export function syncInterfaceViewportProperties(): void {
  if (typeof document === "undefined" || typeof window === "undefined") return;
  const metrics = interfaceViewportMetrics(window.innerWidth, window.innerHeight);
  const style = document.documentElement.style;
  style.setProperty(INTERFACE_LAYOUT_WIDTH_PROPERTY, `${metrics.width}px`);
  style.setProperty(INTERFACE_LAYOUT_HEIGHT_PROPERTY, `${metrics.height}px`);
  style.setProperty(INTERFACE_OVERLAY_GUTTER_PROPERTY, `${metrics.overlayGutter}px`);
  window.dispatchEvent(new Event(INTERFACE_LAYOUT_CHANGE_EVENT));
}

export async function applyInterfaceScale(value: number): Promise<void> {
  const { scale, inverse } = interfaceScaleFactors(value);
  document.documentElement.style.setProperty(INTERFACE_SCALE_PROPERTY, String(scale));
  document.documentElement.style.setProperty(
    INTERFACE_SCALE_INVERSE_PROPERTY,
    String(inverse),
  );
  syncInterfaceViewportProperties();
}
