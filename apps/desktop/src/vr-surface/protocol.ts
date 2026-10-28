import type { ControlItem, ControlSnapshot } from "../vr-controls/protocol";
import type { VrDashboardViewModel } from "../integrations/vr-overlay-native";
import type { DashboardCommand, DashboardUi } from "./dashboard-protocol";
import type { OcrScene } from "./ocr-scene";
import type { PlayerAudioPrompt } from "../capture/player-audio-vr";
import type { PlayerDisplayView, WristMenuCommand, WristMenuUi } from "../vr-controls/player-display-protocol";

export type Surface = keyof typeof SURFACE_SIZE;
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

export interface CaptionOriginal {
  source: string;
  track_id: string | null;
  text: string;
  language: string | null;
  translation_sources: string[];
  live: boolean;
}
export interface CaptionMessage { text: string; side: "left" | "right"; speaker?: string | null; original?: CaptionOriginal | null }
export interface CaptionVisibility { hidden_tracks: readonly string[]; hidden_source_languages: readonly string[] }
interface CaptionSelection { visibility?: CaptionVisibility; max_visible_players?: number | null; max_entries?: number }
export interface CaptionLabels { me: string; other: string }

export type SurfaceView =
  | { kind: "menu"; controls: ControlSnapshot; players?: PlayerDisplayView; ui?: WristMenuUi; hold_progress?: number | null }
  | ({ kind: "dialogue"; controls: ControlSnapshot; messages: CaptionMessage[]; notice?: string | null; labels?: CaptionLabels; mode: "wrist" | "world"; follow_view: boolean; busy: boolean; error: string | null; font_size: number; opacity: number } & CaptionSelection)
  | { kind: "reader"; dark: boolean; labels: Record<string, string>; reader: ReaderView; font_size: number; opacity: number }
  | ({ kind: "headset"; dark: boolean; messages: CaptionMessage[]; notice?: string | null; labels?: CaptionLabels; lines_per_language: number; font_size: number; opacity: number } & CaptionSelection)
  | { kind: "ocr_progress"; dark: boolean; progress: ProgressView | null }
  | { kind: "player_audio"; dark: boolean; notice: PlayerAudioPrompt }
  | { kind: "dashboard"; dark: boolean; dashboard: VrDashboardViewModel; ui: DashboardUi }
  | { kind: "dashboard_thumbnail" | "ocr_selection"; dark: boolean }
  | { kind: "ocr_result"; dark: boolean; scene: OcrScene };

export interface ProgressView {
  scan_id: number;
  status: string;
  detail: string;
  elapsed_seconds: number;
  stages: ("pending" | "active" | "complete" | "skipped" | "failed")[];
  stage_labels: string[];
  translation_fraction: [number, number] | null;
}

export interface SurfaceState {
  key: SurfaceKey;
  scroll_key?: SurfaceKey;
  view: SurfaceView;
  hovered: number | null;
  pressed: number | null;
  pointer: [number, number] | null;
  activity?: SurfaceActivity;
  restore_scroll?: number | null;
}

export interface SurfaceActivity { active: boolean; request: number }
export interface SurfacePointerState { key: SurfaceKey; point: [number, number] | null }
export interface SurfaceScrollState { key: SurfaceKey; delta: number }

export interface SurfaceLayout {
  key: SurfaceKey;
  regions: { id: number; bounds: [number, number, number, number]; circle?: boolean; dashboard?: DashboardCommand; menu?: WristMenuCommand }[];
  menu_bounds?: [number, number, number, number];
  panels?: { bounds: [number, number, number, number]; radius: [number, number] }[];
  page_count: number;
  unplaced?: number[];
}

export const SURFACE_SIZE = { menu: [1280, 1280], dialogue: [768, 768], reader: [1024, 1024], headset: [1024, 1360], ocr_progress: [768, 224], dashboard: [1440, 900], dashboard_thumbnail: [256, 256], ocr_selection: [256, 256], ocr_result: [2048, 2048], player_audio: [1024, 1024] } as const;

export function isSurface(value: string | null): value is Surface {
  return value !== null && Object.hasOwn(SURFACE_SIZE, value);
}

export function hasPointer(surface: Surface): boolean {
  return surface === "menu" || surface === "dialogue" || surface === "reader" || surface === "dashboard" || surface === "player_audio";
}

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
export function markerBits(key: SurfaceKey, paintSerial: number): number[] {
  return [key.generation, key.revision, paintSerial, ~paintSerial].flatMap((word) => Array.from({ length: 32 }, (_, index) => (word >>> (31 - index)) & 1));
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
