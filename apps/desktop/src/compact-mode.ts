import type { Subtitle } from "./types";

export const COMPACT_WINDOW_SIZE = { width: 720, height: 120 } as const;
export const COMPACT_LOOKUP_WINDOW_SIZE = { width: 720, height: 520 } as const;
export const COMPACT_WINDOW_MIN_WIDTH = 480;

export function compactWindowSize(
  lookupOpen: boolean,
  width: number = COMPACT_WINDOW_SIZE.width,
) {
  const height = lookupOpen
    ? COMPACT_LOOKUP_WINDOW_SIZE.height
    : COMPACT_WINDOW_SIZE.height;
  return { width, height };
}

export function compactWindowConstraints(lookupOpen: boolean) {
  const { height } = compactWindowSize(lookupOpen);
  return {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: height,
    maxHeight: height,
  };
}

export function subtitleForCompactView(subtitles: Subtitle[], lookupContext?: string) {
  if (!lookupContext) return subtitles[0];
  return subtitles.find((subtitle) => subtitle.text === lookupContext) ?? subtitles[0];
}
