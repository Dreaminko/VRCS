import { useSyncExternalStore } from "react";
import type { SpeakerIdentity } from "./types";

// Late metadata updates both previews and saved rows without replaying text.
const speakers = new Map<string, SpeakerIdentity>();
const listeners = new Set<() => void>();

export function publishSpeakerIdentity(speaker: SpeakerIdentity) {
  speakers.delete(speaker.id);
  speakers.set(speaker.id, speaker);
  if (speakers.size > 2_000) speakers.delete(speakers.keys().next().value!);
  listeners.forEach((listener) => listener());
}

export function clearSpeakerIdentities() {
  if (!speakers.size) return;
  speakers.clear();
  listeners.forEach((listener) => listener());
}

export function useSpeakerIdentity(speaker: SpeakerIdentity | null | undefined) {
  const update = useSyncExternalStore(
    (listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    () => speaker ? speakers.get(speaker.id) : undefined,
    () => undefined,
  );
  return update ? { ...speaker!, display_name: update.display_name, user_id: update.user_id ?? speaker?.user_id } : speaker;
}
