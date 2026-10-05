import type { Subtitle } from "./subtitles/types";
import type { LiveTranscription } from "./capture/types";
import { livePartialHasSubtitle } from "./realtime-state.ts";

export const COMPACT_WINDOW_SIZE = { width: 720, height: 120 } as const;
export const COMPACT_PANEL_WINDOW_SIZE = { width: 720, height: 520 } as const;
export const COMPACT_WINDOW_MIN_WIDTH = 480;
export const COMPACT_WINDOW_MAX_HEIGHT = 360;
export const COMPACT_SUBTITLE_HEIGHT_STEP = 60;
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
