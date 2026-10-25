import { useMemo, useSyncExternalStore } from "react";

import type {
  AudioLevel,
  LiveTranscription,
} from "../capture/types";
import type { Subtitle } from "../subtitles/types";

type Listener = () => void;
type TranslationPartial = NonNullable<Subtitle["translation_partial"]> & { preferred: boolean };

const audioLevels = new Map<AudioLevel["source"], AudioLevel>();
const audioLevelListeners = new Set<Listener>();
const livePartials = new Map<LiveTranscription["source"], LiveTranscription[]>();
const EMPTY_LIVE_PARTIALS: LiveTranscription[] = [];
const livePartialListeners = new Map<LiveTranscription["source"], Set<Listener>>();
const terminatedLivePartialIds = new Set<string>();
const terminatedLivePartialOrder: string[] = [];
const MAX_TERMINATED_LIVE_PARTIALS = 1_024;
const translationPartials = new Map<number, TranslationPartial[]>();
const translationPartialListeners = new Map<number, Set<Listener>>();

function notify(listeners: Set<Listener>) {
  listeners.forEach((listener) => listener());
}

function subscribe(listeners: Set<Listener>, listener: Listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
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
  notify(audioLevelListeners);
}

export function clearAudioLevels() {
  if (!audioLevels.size) return;
  audioLevels.clear();
  notify(audioLevelListeners);
}

export function useAudioLevel(source: AudioLevel["source"]): AudioLevel | null {
  return useSyncExternalStore(
    (listener) => subscribe(audioLevelListeners, listener),
    () => audioLevels.get(source) ?? null,
    () => null,
  );
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
  // One preview per player, plus the legacy unlabelled source. Arrays are stable
  // between updates for useSyncExternalStore. Completion/close events remove
  // previews; do not silently hide players when capture has no concurrency cap.
  livePartials.set(partial.source, [
    ...previous.filter((item) => (item.speaker?.id ?? null) !== (partial.speaker?.id ?? null)),
    partial,
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

export function getLivePartial(
  source: LiveTranscription["source"],
): LiveTranscription | null {
  return livePartials.get(source)?.at(-1) ?? null;
}

export function getLivePartials(source: LiveTranscription["source"]): LiveTranscription[] {
  return livePartials.get(source) ?? EMPTY_LIVE_PARTIALS;
}

export function useLivePartials(source: LiveTranscription["source"]): LiveTranscription[] {
  return useSyncExternalStore(
    (listener) => subscribeKey(livePartialListeners, source, listener),
    () => getLivePartials(source),
    () => EMPTY_LIVE_PARTIALS,
  );
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

export function useLivePartial(
  source: LiveTranscription["source"],
): LiveTranscription | null {
  return useSyncExternalStore(
    (listener) => subscribeKey(livePartialListeners, source, listener),
    () => getLivePartial(source),
    () => null,
  );
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

export function useTranslationPartials(
  subtitleId: number | null,
): TranslationPartial[] {
  return useSyncExternalStore(
    (listener) => subtitleId === null
      ? () => undefined
      : subscribeKey(translationPartialListeners, subtitleId, listener),
    () => subtitleId === null
      ? EMPTY_TRANSLATION_PARTIALS
      : translationPartials.get(subtitleId) ?? EMPTY_TRANSLATION_PARTIALS,
    () => EMPTY_TRANSLATION_PARTIALS,
  );
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
