import type { Subtitle } from "../subtitles/types";
import type { LiveTranscription } from "../capture/types";
import { subtitleSourceLanguage } from "../subtitles/player-history.ts";

export const COMPACT_WINDOW_SIZE = { width: 720, height: 160 } as const;
export const COMPACT_PANEL_WINDOW_SIZE = { width: 720, height: 520 } as const;
export const COMPACT_WINDOW_MIN_WIDTH = 480;
export const COMPACT_WINDOW_MAX_HEIGHT = 720;

export function compactPreviewText(text: string): string {
  return text.trimEnd();
}

export function compactPartialMatchesSubtitle(partial: LiveTranscription, subtitle: Subtitle): boolean {
  const utteranceId = partial.source_utterance_id ?? partial.utterance_id;
  return Boolean(utteranceId && subtitle.utterance_id === utteranceId
    && (subtitle.source ?? "speaker") === partial.source);
}

export function compactLivePreview(
  partial: LiveTranscription | null,
  subtitles: Subtitle[],
): LiveTranscription | null {
  if (!partial) return partial;
  // A completed original can arrive without any source preview deltas.
  const completed = subtitles.find((subtitle) => compactPartialMatchesSubtitle(partial, subtitle));
  const text = partial.text.trim() ? partial.text : partial.completed_original?.trim() ? partial.completed_original : completed?.text ?? partial.text;
  // A final source may supply language metadata after preview deltas started.
  const language = completed ? subtitleSourceLanguage(completed) ?? partial.language : partial.language;
  return text === partial.text && language === partial.language ? partial : { ...partial, text, language };
}

// A number reserves the measured Chatbox height; true opens a selection/dialog panel.
export type CompactPanelState = boolean | number;

export function compactChatboxHeight(panelState: CompactPanelState): number {
  return typeof panelState === "number"
    ? Math.min(COMPACT_WINDOW_MAX_HEIGHT - COMPACT_WINDOW_SIZE.height, Math.max(0, Math.ceil(panelState)))
    : 0;
}

export function compactWindowSize(
  panelState: CompactPanelState,
  width: number = COMPACT_WINDOW_SIZE.width,
  height: number = COMPACT_WINDOW_SIZE.height,
) {
  return {
    width,
    height: panelState === true
      ? COMPACT_PANEL_WINDOW_SIZE.height
      : clampCompactWindowHeight(height + compactChatboxHeight(panelState)),
  };
}

export function compactWindowConstraints(panelState: CompactPanelState) {
  if (panelState !== true) {
    return {
      minWidth: COMPACT_WINDOW_MIN_WIDTH,
      minHeight: COMPACT_WINDOW_SIZE.height + compactChatboxHeight(panelState),
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

export function subtitlesForCompactView(
  subtitles: Subtitle[],
  selectionContext?: string,
): Subtitle[] {
  if (selectionContext) {
    const selected = subtitles.find((subtitle) => subtitle.text === selectionContext)
      ?? subtitles[0];
    return selected ? [selected] : [];
  }

  // The layout layer bounds measurement work and chooses the visible suffix.
  return subtitles.slice().reverse();
}
