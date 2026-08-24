import type { Subtitle } from "./subtitles/types";

export const COMPACT_WINDOW_SIZE = { width: 720, height: 120 } as const;
export const COMPACT_PANEL_WINDOW_SIZE = { width: 720, height: 520 } as const;
export const COMPACT_WINDOW_MIN_WIDTH = 480;
export const COMPACT_WINDOW_MAX_HEIGHT = 360;
export const COMPACT_SUBTITLE_HEIGHT_STEP = 60;
export const COMPACT_SUBTITLE_MAX_ITEMS = 4;

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
  const steps = Math.floor(
    (clampCompactWindowHeight(height) - COMPACT_WINDOW_SIZE.height)
      / COMPACT_SUBTITLE_HEIGHT_STEP,
  );
  return Math.min(COMPACT_SUBTITLE_MAX_ITEMS, steps + 1);
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
