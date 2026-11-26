import { isOtherPlayerSource, originalSourceLanguage, sourceLanguageVisible, trackVisible } from "../subtitles/player-history.ts";
import type { CaptionMessage, SurfaceView } from "./protocol.ts";

type CaptionView = Pick<Extract<SurfaceView, { kind: "headset" | "dialogue" }>, "kind" | "messages" | "visibility" | "max_visible_players" | "max_entries">;
const live = (message: CaptionMessage) => message.original?.live === true;
const other = (message: CaptionMessage) => message.original ? isOtherPlayerSource(message.original.source) : message.side === "left";
const lane = (message: CaptionMessage) => message.original
  ? `${message.original.source}:${message.original.track_id ?? "unidentified"}` : `${message.side}:${message.speaker ?? "sample"}`;
const player = (message: CaptionMessage) => message.original?.track_id ?? message.speaker ?? "unidentified";

export function visibleCaptionMessages(view: CaptionView): CaptionMessage[] {
  const hidden = view.visibility;
  let messages: CaptionMessage[] = [];
  for (const message of view.messages) {
    if (!message.text.trim()) continue;
    const original = message.original;
    if (original && hidden) {
      const speaker = original.track_id ? { id: original.track_id, index: 0 } : null;
      if (!trackVisible(original.source, speaker, hidden.hidden_tracks)) continue;
      if (hidden.hidden_source_languages.length && isOtherPlayerSource(original.source)
        && !sourceLanguageVisible(original.source,
          originalSourceLanguage(original.language, original.translation_sources, original.detected_language),
          hidden.hidden_source_languages)) continue;
    }
    messages.push(message);
  }

  if (view.kind === "headset") {
    if (view.max_visible_players === null) {
      // Mixed audio still uses one caption; remote text keeps independent player lanes.
      let latestLive: CaptionMessage | undefined;
      let latestAny: CaptionMessage | undefined;
      const text = new Map<string, CaptionMessage>();
      for (const message of messages) {
        if (message.original?.source === "vrchat_chatbox") {
          text.set(lane(message), message);
        } else {
          latestAny = message;
          if (live(message)) latestLive = message;
        }
      }
      const latest = latestLive ?? latestAny;
      return latest ? [latest, ...text.values()] : [...text.values()];
    }
    const lanes = new Map<string, CaptionMessage>();
    for (const message of messages) if (!live(message)) lanes.set(lane(message), message);
    for (const message of messages) if (live(message)) lanes.set(lane(message), message);
    messages = [...lanes.values()].sort((a, b) => Number(live(b)) - Number(live(a)));
  }

  const limit = view.max_visible_players;
  if (limit != null && limit > 0) {
    const selected = new Set<string>();
    // Keep active players first, then fill unused slots from recent history.
    for (const message of messages) {
      if (selected.size >= limit) break;
      if (live(message) && other(message)) selected.add(player(message));
    }
    for (let i = messages.length - 1; i >= 0 && selected.size < limit; i--) {
      const message = messages[i]!;
      if (!live(message) && other(message)) selected.add(player(message));
    }
    messages = messages.filter(message => !other(message) || selected.has(player(message)));
  }
  if (view.kind === "dialogue") {
    const count = Number.isFinite(view.max_entries) ? Math.min(10, Math.max(3, Math.floor(view.max_entries!))) : 10;
    if (messages.length > count) messages = messages.slice(-count);
  }
  return messages;
}
