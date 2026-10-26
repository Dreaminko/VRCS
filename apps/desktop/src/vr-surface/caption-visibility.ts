import { originalSourceLanguage, sourceLanguageVisible, trackVisible } from "../subtitles/player-history.ts";
import type { CaptionMessage, SurfaceView } from "./protocol.ts";

type CaptionView = Pick<Extract<SurfaceView, { kind: "headset" | "dialogue" }>, "kind" | "messages" | "visibility" | "max_visible_players" | "max_entries">;
const live = (message: CaptionMessage) => message.original?.live === true;
const other = (message: CaptionMessage) => message.original ? message.original.source === "speaker" : message.side === "left";
const lane = (message: CaptionMessage) => message.original
  ? `${message.original.source}:${message.original.track_id ?? "unidentified"}` : `${message.side}:${message.speaker ?? "sample"}`;

export function visibleCaptionMessages(view: CaptionView): CaptionMessage[] {
  const hidden = view.visibility;
  let messages = view.messages.filter(message => {
    if (!message.text.trim()) return false;
    const original = message.original;
    if (!original || !hidden) return true; // Sample captions have no captured source.
    const speaker = original.track_id ? { id: original.track_id, index: 0 } : null;
    return trackVisible(original.source, speaker, hidden.hidden_tracks)
      && (!hidden.hidden_source_languages.length || original.source !== "speaker" || sourceLanguageVisible(original.source,
        originalSourceLanguage(original.text, original.language, original.translation_sources), hidden.hidden_source_languages));
  });

  if (view.kind === "headset") {
    if (view.max_visible_players === null) {
      // Mixed audio retains one latest visible caption, prioritizing previews.
      const latest = messages.filter(live).at(-1) ?? messages.at(-1);
      return latest ? [latest] : [];
    }
    const lanes = new Map<string, CaptionMessage>();
    for (const message of [...messages.filter(message => !live(message)), ...messages.filter(live)]) {
      lanes.set(lane(message), message);
    }
    messages = [...lanes.values()].sort((a, b) => Number(live(b)) - Number(live(a)));
  }

  const limit = view.max_visible_players;
  if (limit != null && limit > 0) {
    const selected = new Set<string>();
    // Keep active players first, then fill unused slots from recent history.
    for (const message of [...messages.filter(live), ...messages.filter(message => !live(message)).reverse()]) {
      if (other(message) && selected.size < limit) selected.add(lane(message));
    }
    messages = messages.filter(message => !other(message) || selected.has(lane(message)));
  }
  if (view.kind === "dialogue") {
    const count = Number.isFinite(view.max_entries) ? Math.min(10, Math.max(3, Math.floor(view.max_entries!))) : 10;
    messages = messages.slice(-count);
  }
  return messages;
}
