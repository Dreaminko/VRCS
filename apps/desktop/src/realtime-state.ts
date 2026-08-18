import { useSyncExternalStore } from "react";

import type { AudioLevel, LiveTranscription, Subtitle } from "./types";

type Listener = () => void;
type TranslationPartial = NonNullable<Subtitle["translation_partial"]>;

const audioLevels = new Map<AudioLevel["source"], AudioLevel>();
const audioLevelListeners = new Set<Listener>();
const livePartials = new Map<LiveTranscription["source"], LiveTranscription>();
const livePartialListeners = new Map<LiveTranscription["source"], Set<Listener>>();
const translationPartials = new Map<number, TranslationPartial>();
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

export function publishLivePartial(partial: LiveTranscription) {
  livePartials.set(partial.source, partial);
  notifyKey(livePartialListeners, partial.source);
}

export function completeLivePartial(
  source: LiveTranscription["source"],
  utteranceId: string,
) {
  if (livePartials.get(source)?.utterance_id !== utteranceId) return;
  livePartials.delete(source);
  notifyKey(livePartialListeners, source);
}

export function clearLivePartial(source: LiveTranscription["source"]) {
  if (!livePartials.delete(source)) return;
  notifyKey(livePartialListeners, source);
}

export function clearLivePartials() {
  if (!livePartials.size) return;
  const sources = [...livePartials.keys()];
  livePartials.clear();
  sources.forEach((source) => notifyKey(livePartialListeners, source));
}

export function getLivePartial(
  source: LiveTranscription["source"],
): LiveTranscription | null {
  return livePartials.get(source) ?? null;
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
  translationPartials.set(subtitleId, partial);
  notifyKey(translationPartialListeners, subtitleId);
}

export function clearTranslationPartial(subtitleId: number) {
  if (!translationPartials.delete(subtitleId)) return;
  notifyKey(translationPartialListeners, subtitleId);
}

export function clearTranslationPartials() {
  if (!translationPartials.size) return;
  const subtitleIds = [...translationPartials.keys()];
  translationPartials.clear();
  subtitleIds.forEach((subtitleId) => notifyKey(translationPartialListeners, subtitleId));
}

export function useTranslationPartial(
  subtitleId: number | null,
): TranslationPartial | null {
  return useSyncExternalStore(
    (listener) => subtitleId === null
      ? () => undefined
      : subscribeKey(translationPartialListeners, subtitleId, listener),
    () => subtitleId === null ? null : translationPartials.get(subtitleId) ?? null,
    () => null,
  );
}
