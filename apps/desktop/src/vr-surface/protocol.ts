import type { ControlItem, ControlSnapshot } from "../vr-controls/protocol";

export type Surface = "menu" | "dialogue" | "reader";
export interface SurfaceKey { generation: number; revision: number }

export interface ReaderView {
  mode: "ocr" | "vision";
  source: string;
  translation: string;
  translation_label: string;
  title: string;
  status: string;
  detail: string;
  block_index: number;
  block_count: number;
  page: number;
  pinned: boolean;
  source_only: boolean;
  language_count: number;
  hovered?: string | null;
}

export type SurfaceView =
  | { kind: "menu"; controls: ControlSnapshot }
  | { kind: "dialogue"; controls: ControlSnapshot; messages: { text: string; side: "left" | "right" }[]; mode: "wrist" | "world"; busy: boolean; error: string | null; font_size: number; opacity: number }
  | { kind: "reader"; dark: boolean; labels: Record<string, string>; reader: ReaderView; font_size: number; opacity: number };

export interface SurfaceState {
  key: SurfaceKey;
  view: SurfaceView;
  hovered: number | null;
  pressed: number | null;
  pointer: [number, number] | null;
}

export interface SurfacePointerState { key: SurfaceKey; point: [number, number] | null }

export interface SurfaceLayout {
  key: SurfaceKey;
  regions: { id: number; bounds: [number, number, number, number]; circle?: boolean }[];
  page_count: number;
}

export const SURFACE_SIZE = { menu: [1024, 256], dialogue: [768, 768], reader: [1024, 1024] } as const;

export function menuStatus(item: ControlItem, labels: Record<string, string>) {
  const status = item.busy ? "busy" : item.reason || !item.available ? "unavailable" : item.requested && !item.effective ? "waiting" : "idle";
  const detail = status === "busy" ? labels.controlBusy || "Working"
    : status === "waiting" ? labels.waitingContent || "Enabled, waiting for content" : item.reason;
  return { status, label: detail ? `${item.label} · ${detail}` : item.label };
}

export function sameKey(a: SurfaceKey | null, b: SurfaceKey): boolean {
  return a?.generation === b.generation && a.revision === b.revision;
}

export function newerKey(next: SurfaceKey, current: SurfaceKey | null): boolean {
  if (![next.generation, next.revision].every((value) => Number.isInteger(value) && value >= 0 && value <= 0xffffffff)) return false;
  return !current || next.generation > current.generation || (next.generation === current.generation && next.revision > current.revision);
}

// WGC reads this small strip before accepting the matching texture and hit map.
export function markerBits(key: SurfaceKey): number[] {
  return [key.generation, key.revision].flatMap((word) => Array.from({ length: 32 }, (_, index) => (word >>> (31 - index)) & 1));
}

export function pageGeometry(height: number, viewport: number, requested: number, lineHeight: number) {
  if (![height, viewport, lineHeight].every(Number.isFinite) || viewport <= 0) return { count: 1, page: 0, offset: 0 };
  const overflow = Math.max(0, height - viewport);
  const overlap = Math.min(viewport / 3, Math.max(1, lineHeight) * 2);
  const stride = Math.max(1, viewport - overlap);
  const count = Math.max(1, Math.ceil(overflow / stride) + 1);
  const page = Math.max(0, Math.min(count - 1, Number.isFinite(requested) ? Math.floor(requested) : 0));
  return { count, page, offset: Math.min(overflow, page * stride) };
}
