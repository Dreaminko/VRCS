import { useCallback, useMemo, useSyncExternalStore } from "react";
import type { SpeakerIdentity } from "./types";

// Late metadata updates both previews and saved rows without replaying text.
const speakers = new Map<string, SpeakerIdentity>();
const allListeners = new Set<() => void>();
const speakerListeners = new Map<string, Set<() => void>>();
let revision = 0;
const subscribeAll = (listener: () => void) => {
  allListeners.add(listener);
  return () => { allListeners.delete(listener); };
};
const subscribeSpeaker = (speakerId: string, listener: () => void) => {
  let set = speakerListeners.get(speakerId);
  if (!set) {
    set = new Set();
    speakerListeners.set(speakerId, set);
  }
  set.add(listener);
  return () => {
    set.delete(listener);
    if (!set.size) speakerListeners.delete(speakerId);
  };
};
const noopSubscribe = () => () => {};
const getRevision = () => revision;
const zeroRevision = () => 0;
const undefinedSnapshot = () => undefined;

export function useSpeakerIdentities(identities: readonly SpeakerIdentity[]): SpeakerIdentity[] {
  const currentRevision = useSyncExternalStore(subscribeAll, getRevision, zeroRevision);
  return useMemo(() => identities.map(identity => {
    const update = speakers.get(identity.id);
    return update ? { ...identity, display_name: update.display_name, user_id: update.user_id ?? identity.user_id } : identity;
  }), [identities, currentRevision]);
}

export function publishSpeakerIdentity(speaker: SpeakerIdentity) {
  const previous = speakers.get(speaker.id);
  if (
    previous
    && previous.display_name === speaker.display_name
    && previous.user_id === speaker.user_id
    && previous.index === speaker.index
  ) {
    return;
  }
  speakers.delete(speaker.id);
  speakers.set(speaker.id, speaker);
  if (speakers.size > 2_000) speakers.delete(speakers.keys().next().value!);
  revision++;
  allListeners.forEach((listener) => listener());
  speakerListeners.get(speaker.id)?.forEach((listener) => listener());
}

export function clearSpeakerIdentities() {
  if (!speakers.size) return;
  speakers.clear();
  revision++;
  allListeners.forEach((listener) => listener());
  speakerListeners.forEach((listeners) => listeners.forEach((listener) => listener()));
}

export function useSpeakerIdentity(speaker: SpeakerIdentity | null | undefined) {
  const speakerId = speaker?.id;
  const subscribe = useCallback(
    (listener: () => void) => (speakerId ? subscribeSpeaker(speakerId, listener) : noopSubscribe()),
    [speakerId],
  );
  const getSnapshot = useCallback(
    () => (speakerId ? speakers.get(speakerId) : undefined),
    [speakerId],
  );
  const update = useSyncExternalStore(subscribe, getSnapshot, undefinedSnapshot);
  return useMemo(
    () => (update && speaker
      ? { ...speaker, display_name: update.display_name, user_id: update.user_id ?? speaker.user_id }
      : speaker),
    [speaker, update],
  );
}
