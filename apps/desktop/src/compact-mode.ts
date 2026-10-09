import type { Subtitle } from "./subtitles/types";
import type { LiveTranscription } from "./capture/types";
import { livePartialHasSubtitle } from "./realtime-state.ts";

export const COMPACT_WINDOW_SIZE = { width: 720, height: 160 } as const;
export const COMPACT_PANEL_WINDOW_SIZE = { width: 720, height: 520 } as const;
export const COMPACT_WINDOW_MIN_WIDTH = 480;
export const COMPACT_WINDOW_MAX_HEIGHT = 720;
// Root borders, header, and body vertical padding; keep in sync with compact.css.
const COMPACT_CONTENT_VERTICAL_INSET = 2 + 32 + 8 + 12;
// Add history only when two lines per language fit using the largest line
// height in either mode. Three lines is a scroll limit, not a minimum reserve.
// Include the gap, row padding, and separator in the whole-pixel budget.
export const COMPACT_SUBTITLE_HEIGHT_STEP = 2 * 24 * 2 + 4 + 6 + 2;
export const COMPACT_SUBTITLE_MAX_ITEMS = 4;
export const COMPACT_PREVIEW_MAX_CHARS = 600;

export function compactPreviewText(text: string): string {
  // Bound layout work for complete paragraphs; the viewport shows the newest lines.
  return Array.from(text.trimEnd()).slice(-COMPACT_PREVIEW_MAX_CHARS).join("");
}

export function compactLivePreview(
  partial: LiveTranscription | null,
  subtitles: Subtitle[],
): LiveTranscription | null {
  if (!partial || partial.text.trim()) return partial;
  if (partial.completed_original?.trim()) return { ...partial, text: partial.completed_original };
  // A completed original can arrive without any source preview deltas.
  const completed = subtitles.find((subtitle) => livePartialHasSubtitle(partial, [subtitle]));
  if (!completed) return partial;
  return { ...partial, text: completed.text, language: completed.language ?? partial.language };
}

export type CompactPanelState = boolean;

export function compactWindowSize(
  panelState: CompactPanelState,
  width: number = COMPACT_WINDOW_SIZE.width,
  height: number = COMPACT_WINDOW_SIZE.height,
) {
  return {
    width,
    height: panelState
      ? COMPACT_PANEL_WINDOW_SIZE.height
      : clampCompactWindowHeight(height),
  };
}

export function compactWindowConstraints(panelState: CompactPanelState) {
  if (!panelState) {
    return {
      minWidth: COMPACT_WINDOW_MIN_WIDTH,
      minHeight: COMPACT_WINDOW_SIZE.height,
      maxHeight: COMPACT_WINDOW_MAX_HEIGHT,
    };
  }

  return {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: COMPACT_PANEL_WINDOW_SIZE.height,
    maxHeight: COMPACT_PANEL_WINDOW_SIZE.height,
  };
}

export function clampCompactWindowHeight(height: number): number {
  return Math.min(
    COMPACT_WINDOW_MAX_HEIGHT,
    Math.max(COMPACT_WINDOW_SIZE.height, Math.round(height)),
  );
}

export function compactSubtitleCount(height: number): number {
  const capacity = Math.floor(
    (clampCompactWindowHeight(height) - COMPACT_CONTENT_VERTICAL_INSET)
      / COMPACT_SUBTITLE_HEIGHT_STEP,
  );
  // Capacity depends on viewport height, so streaming text and mode changes
  // cannot repeatedly add and remove a history row as their content grows.
  return Math.min(COMPACT_SUBTITLE_MAX_ITEMS, Math.max(1, capacity));
}

export function subtitlesForCompactView(
  subtitles: Subtitle[],
  height: number,
  selectionContext?: string,
): Subtitle[] {
  if (selectionContext) {
    const selected = subtitles.find((subtitle) => subtitle.text === selectionContext)
      ?? subtitles[0];
    return selected ? [selected] : [];
  }

  return subtitles.slice(0, compactSubtitleCount(height)).reverse();
}
