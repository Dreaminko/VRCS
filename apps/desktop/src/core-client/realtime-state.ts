import { useCallback, useMemo, useSyncExternalStore } from "react";

import type {
  AudioLevel,
  LiveTranscription,
} from "../capture/types";
import type { Subtitle } from "../subtitles/types";

type Listener = () => void;
type TranslationPartial = NonNullable<Subtitle["translation_partial"]> & { preferred: boolean };

const audioLevels = new Map<AudioLevel["source"], AudioLevel>();
const audioLevelListeners = new Map<AudioLevel["source"], Set<Listener>>();
const livePartials = new Map<LiveTranscription["source"], LiveTranscription[]>();
const EMPTY_LIVE_PARTIALS: LiveTranscription[] = [];
const livePartialListeners = new Map<LiveTranscription["source"], Set<Listener>>();
const terminatedLivePartialIds = new Set<string>();
const terminatedLivePartialOrder: string[] = [];
const MAX_TERMINATED_LIVE_PARTIALS = 1_024;
const translationPartials = new Map<number, TranslationPartial[]>();
const translationPartialListeners = new Map<number, Set<Listener>>();
const nullSnapshot = () => null;
const emptyLivePartialsSnapshot = () => EMPTY_LIVE_PARTIALS;

function notify(listeners: Set<Listener>) {
  listeners.forEach((listener) => listener());
}

function subscribeKey<Key>(
  listenersByKey: Map<Key, Set<Listener>>,
  key: Key,
  listener: Listener,
) {
  const listeners = listenersByKey.get(key) ?? new Set<Listener>();
  listeners.add(listener);
  listenersByKey.set(key, listeners);
  return () => {
    listeners.delete(listener);
    if (!listeners.size) listenersByKey.delete(key);
  };
}

function notifyKey<Key>(listenersByKey: Map<Key, Set<Listener>>, key: Key) {
  const listeners = listenersByKey.get(key);
  if (listeners) notify(listeners);
}

export function publishAudioLevel(level: AudioLevel) {
  audioLevels.set(level.source, level);
  notifyKey(audioLevelListeners, level.source);
}

export function clearAudioLevels() {
  if (!audioLevels.size) return;
  const sources = [...audioLevels.keys()];
  audioLevels.clear();
  sources.forEach((source) => notifyKey(audioLevelListeners, source));
}

export function subscribeAudioLevel(
  source: AudioLevel["source"],
  listener: (level: AudioLevel | null) => void,
): () => void {
  return subscribeKey(audioLevelListeners, source, () => {
    listener(audioLevels.get(source) ?? null);
  });
}

export function useAudioLevel(source: AudioLevel["source"], enabled = true): AudioLevel | null {
  const subscribeSource = useCallback(
    (listener: Listener) => (enabled
      ? subscribeKey(audioLevelListeners, source, listener)
      : () => undefined),
    [enabled, source],
  );
  const getSnapshot = useCallback(
    () => (enabled ? audioLevels.get(source) ?? null : null),
    [enabled, source],
  );
  return useSyncExternalStore(subscribeSource, getSnapshot, nullSnapshot);
}

function livePartialKey(source: LiveTranscription["source"], utteranceId: string) {
  return `${source}:${utteranceId}`;
}

function rememberLivePartialTermination(
  source: LiveTranscription["source"],
  utteranceId: string,
) {
  const key = livePartialKey(source, utteranceId);
  if (terminatedLivePartialIds.has(key)) return;
  terminatedLivePartialIds.add(key);
  terminatedLivePartialOrder.push(key);
  if (terminatedLivePartialOrder.length > MAX_TERMINATED_LIVE_PARTIALS) {
    const expired = terminatedLivePartialOrder.shift();
    if (expired !== undefined) terminatedLivePartialIds.delete(expired);
  }
}

export function publishLivePartial(partial: LiveTranscription) {
  if (terminatedLivePartialIds.has(livePartialKey(partial.source, partial.utterance_id))) {
    return;
  }
  const previous = livePartials.get(partial.source) ?? EMPTY_LIVE_PARTIALS;
  const sameLane = (item: LiveTranscription) => partial.source === "microphone"
    || (item.speaker?.id ?? null) === (partial.speaker?.id ?? null);
  const current = previous.find((item) => item.utterance_id === partial.utterance_id && sameLane(item));
  // One preview per player, plus the legacy unlabelled source. Arrays are stable
  // between updates for useSyncExternalStore. Completion/close events remove
  // previews; do not silently hide players when capture has no concurrency cap.
  livePartials.set(partial.source, [
    ...previous.filter((item) => !sameLane(item)),
    { ...partial, received_at: current?.received_at ?? Date.now() },
  ]);
  notifyKey(livePartialListeners, partial.source);
}

export function completeLivePartial(
  source: LiveTranscription["source"],
  utteranceId: string,
) {
  rememberLivePartialTermination(source, utteranceId);
  const previous = livePartials.get(source);
  if (!previous?.some((item) => item.utterance_id === utteranceId)) return;
  const next = previous.filter((item) => item.utterance_id !== utteranceId);
  if (next.length) livePartials.set(source, next);
  else livePartials.delete(source);
  notifyKey(livePartialListeners, source);
}

export function clearLivePartial(source: LiveTranscription["source"]) {
  if (!livePartials.delete(source)) return;
  notifyKey(livePartialListeners, source);
}

export function resetLivePartial(source: LiveTranscription["source"]) {
  const deleted = livePartials.delete(source);
  const prefix = `${source}:`;
  for (let index = terminatedLivePartialOrder.length - 1; index >= 0; index -= 1) {
    const key = terminatedLivePartialOrder[index];
    if (key?.startsWith(prefix)) {
      terminatedLivePartialOrder.splice(index, 1);
      terminatedLivePartialIds.delete(key);
    }
  }
  if (deleted) notifyKey(livePartialListeners, source);
}

export function clearLivePartials() {
  const sources = [...livePartials.keys()];
  livePartials.clear();
  terminatedLivePartialIds.clear();
  terminatedLivePartialOrder.length = 0;
  sources.forEach((source) => notifyKey(livePartialListeners, source));
}

export function getLivePartials(source: LiveTranscription["source"]): LiveTranscription[] {
  return livePartials.get(source) ?? EMPTY_LIVE_PARTIALS;
}

export function useLivePartials(source: LiveTranscription["source"]): LiveTranscription[] {
  const subscribeSource = useCallback(
    (listener: Listener) => subscribeKey(livePartialListeners, source, listener),
    [source],
  );
  const getSnapshot = useCallback(() => getLivePartials(source), [source]);
  return useSyncExternalStore(subscribeSource, getSnapshot, emptyLivePartialsSnapshot);
}

export function livePartialHasSubtitle(
  partial: LiveTranscription,
  subtitles: Subtitle[],
): boolean {
  return Boolean(partial.source_utterance_id && subtitles.some((subtitle) =>
    subtitle.utterance_id === partial.source_utterance_id
    && (subtitle.source ?? "speaker") === partial.source,
  ));
}

export function publishTranslationPartial(
  subtitleId: number,
  partial: TranslationPartial,
) {
  const partials = translationPartials.get(subtitleId) ?? [];
  translationPartials.set(
    subtitleId,
    [
      ...partials.filter((item) => item.target_language !== partial.target_language),
      partial,
    ].sort((left, right) => Number(right.preferred) - Number(left.preferred)),
  );
  notifyKey(translationPartialListeners, subtitleId);
}

export function clearTranslationPartial(subtitleId: number, targetLanguage?: string) {
  if (targetLanguage) {
    const partials = translationPartials.get(subtitleId);
    if (!partials?.some((partial) => partial.target_language === targetLanguage)) return;
    const next = partials.filter((partial) => partial.target_language !== targetLanguage);
    if (next.length) translationPartials.set(subtitleId, next);
    else translationPartials.delete(subtitleId);
  } else if (!translationPartials.delete(subtitleId)) return;
  notifyKey(translationPartialListeners, subtitleId);
}

export function clearTranslationPartials() {
  if (!translationPartials.size) return;
  const subtitleIds = [...translationPartials.keys()];
  translationPartials.clear();
  subtitleIds.forEach((subtitleId) => notifyKey(translationPartialListeners, subtitleId));
}

const EMPTY_TRANSLATION_PARTIALS: TranslationPartial[] = [];
const emptyTranslationPartialsSnapshot = () => EMPTY_TRANSLATION_PARTIALS;

export function useTranslationPartials(
  subtitleId: number | null,
): TranslationPartial[] {
  const subscribeSubtitle = useCallback(
    (listener: Listener) => (subtitleId === null
      ? () => undefined
      : subscribeKey(translationPartialListeners, subtitleId, listener)),
    [subtitleId],
  );
  const getSnapshot = useCallback(
    () => (subtitleId === null
      ? EMPTY_TRANSLATION_PARTIALS
      : translationPartials.get(subtitleId) ?? EMPTY_TRANSLATION_PARTIALS),
    [subtitleId],
  );
  return useSyncExternalStore(subscribeSubtitle, getSnapshot, emptyTranslationPartialsSnapshot);
}

/** A stable snapshot for a bounded set of compact-layout candidates. */
export function useTranslationPartialsForSubtitles(subtitleIds: readonly (number | null)[]) {
  // Equivalent ID lists must share a store even when a caller creates a fresh
  // array. Otherwise getSnapshot changes on every render and React keeps retrying.
  const idsKey = JSON.stringify(subtitleIds);
  const store = useMemo(() => {
    const ids: (number | null)[] = JSON.parse(idsKey);
    const read = () => ids.map((id) => id === null
      ? EMPTY_TRANSLATION_PARTIALS
      : translationPartials.get(id) ?? EMPTY_TRANSLATION_PARTIALS);
    let snapshot = read();
    return {
      subscribe(listener: Listener) {
        const unsubscribes = [...new Set(ids)].flatMap((id) => id === null
          ? [] : [subscribeKey(translationPartialListeners, id, listener)]);
        return () => unsubscribes.forEach((unsubscribe) => unsubscribe());
      },
      getSnapshot() {
        const next = read();
        if (next.some((partials, index) => partials !== snapshot[index])) snapshot = next;
        return snapshot;
      },
    };
  }, [idsKey]);
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}
