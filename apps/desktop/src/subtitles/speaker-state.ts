import { useSyncExternalStore } from "react";
import type { SpeakerIdentity } from "./types";

// Late metadata updates both previews and saved rows without replaying text.
const speakers = new Map<string, SpeakerIdentity>();
const listeners = new Set<() => void>();
let revision = 0;
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };

export function useSpeakerIdentities(identities: readonly SpeakerIdentity[]): SpeakerIdentity[] {
  useSyncExternalStore(subscribe, () => revision, () => 0);
  return identities.map(identity => {
    const update = speakers.get(identity.id);
    return update ? { ...identity, display_name: update.display_name, user_id: update.user_id ?? identity.user_id } : identity;
  });
}

export function publishSpeakerIdentity(speaker: SpeakerIdentity) {
  speakers.delete(speaker.id);
  speakers.set(speaker.id, speaker);
  if (speakers.size > 2_000) speakers.delete(speakers.keys().next().value!);
  revision++;
  listeners.forEach((listener) => listener());
}

export function clearSpeakerIdentities() {
  if (!speakers.size) return;
  speakers.clear();
  revision++;
  listeners.forEach((listener) => listener());
}

export function useSpeakerIdentity(speaker: SpeakerIdentity | null | undefined) {
  const update = useSyncExternalStore(
    subscribe,
    () => speaker ? speakers.get(speaker.id) : undefined,
    () => undefined,
  );
  return update ? { ...speaker!, display_name: update.display_name, user_id: update.user_id ?? speaker?.user_id } : speaker;
}
