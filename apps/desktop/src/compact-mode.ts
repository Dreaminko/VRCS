import type { Subtitle } from "./types";

export const COMPACT_WINDOW_SIZE = { width: 720, height: 120 } as const;
export const COMPACT_LOOKUP_WINDOW_SIZE = { width: 720, height: 520 } as const;

export function compactWindowSize(lookupOpen: boolean) {
  return lookupOpen ? COMPACT_LOOKUP_WINDOW_SIZE : COMPACT_WINDOW_SIZE;
}

export function subtitleForCompactView(subtitles: Subtitle[], lookupContext?: string) {
  if (!lookupContext) return subtitles[0];
  return subtitles.find((subtitle) => subtitle.text === lookupContext) ?? subtitles[0];
}
