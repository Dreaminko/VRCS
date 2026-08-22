import type { Subtitle } from "./subtitles/types";

export const COMPACT_WINDOW_SIZE = { width: 720, height: 120 } as const;
export const COMPACT_PANEL_WINDOW_SIZE = { width: 720, height: 520 } as const;
export const COMPACT_WINDOW_MIN_WIDTH = 480;

export type CompactPanelState = boolean;

export function compactWindowSize(
  panelState: CompactPanelState,
  width: number = COMPACT_WINDOW_SIZE.width,
) {
  const height = panelState
    ? COMPACT_PANEL_WINDOW_SIZE.height
    : COMPACT_WINDOW_SIZE.height;
  return { width, height };
}

export function compactWindowConstraints(panelState: CompactPanelState) {
  const { height } = compactWindowSize(panelState);
  return {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: height,
    maxHeight: height,
  };
}

export function subtitleForCompactView(subtitles: Subtitle[], selectionContext?: string) {
  if (!selectionContext) return subtitles[0];
  return subtitles.find((subtitle) => subtitle.text === selectionContext) ?? subtitles[0];
}
