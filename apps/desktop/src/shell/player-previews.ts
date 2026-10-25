import type { LiveTranscription } from "../capture/types";
import type { SpeakerIdentity, Subtitle } from "../subtitles/types";

interface CaptionTrack {
  source?: string;
  speaker?: SpeakerIdentity | null;
}

function compareCaptionTracks(left: CaptionTrack, right: CaptionTrack): number {
  return Number((left.source ?? "speaker") !== "speaker") - Number((right.source ?? "speaker") !== "speaker")
    || (left.speaker?.index ?? 0) - (right.speaker?.index ?? 0)
    || (left.speaker?.id ?? "").localeCompare(right.speaker?.id ?? "")
    || (left.source ?? "speaker").localeCompare(right.source ?? "speaker");
}

/** Input is oldest first, with live previews after completed captions. */
export function latestCaptionsByTrack<T extends CaptionTrack>(captions: readonly T[]): T[] {
  const lanes = new Map<string, T>();
  for (const caption of captions) {
    lanes.set(`${caption.source ?? "speaker"}:${caption.speaker?.id ?? "unidentified"}`, caption);
  }
  return [...lanes.values()].sort(compareCaptionTracks);
}

export function selectPlayerHistory(subtitles: Subtitle[], previews: LiveTranscription[], limit?: number): Subtitle[] {
  if (!limit) return subtitles;
  const selected = new Set(previews.filter(item => item.source !== "microphone").map(item => item.speaker?.id ?? "speaker"));
  for (const item of subtitles.slice().reverse()) {
    if (selected.size >= limit) break;
    if (item.source !== "microphone" && item.source !== "chatbox") selected.add(item.speaker?.id ?? "speaker");
  }
  return subtitles.filter(item => item.source === "microphone" || item.source === "chatbox" || selected.has(item.speaker?.id ?? "speaker"));
}

export function selectPlayerPreviews(speaker: LiveTranscription[], microphone: LiveTranscription[], limit?: number): LiveTranscription[] {
  if (limit === undefined) {
    const latest = microphone.at(-1) ?? speaker.at(-1);
    return latest ? [latest] : [];
  }
  // Stable lane order: token arrival order must not move people around or switch
  // the visible players on every streamed delta.
  const players = speaker.filter(item => item.text.trim() || item.translation?.trim() || item.completed_original?.trim())
    .slice().sort(compareCaptionTracks);
  return [...(limit > 0 ? players.slice(0, limit) : players), ...microphone];
}
