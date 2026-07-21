import type { Subtitle } from "./types";

export function subtitleForCompactView(subtitles: Subtitle[], lookupContext?: string) {
  if (!lookupContext) return subtitles[0];
  return subtitles.find((subtitle) => subtitle.text === lookupContext) ?? subtitles[0];
}
