import { useSyncExternalStore } from "react";

import type { AudioLevel, Subtitle } from "./types";

type Listener = () => void;
type TranslationPartial = NonNullable<Subtitle["translation_partial"]>;

const audioLevels = new Map<AudioLevel["source"], AudioLevel>();
const audioLevelListeners = new Set<Listener>();
const translationPartials = new Map<number, TranslationPartial>();
const translationPartialListeners = new Set<Listener>();

function notify(listeners: Set<Listener>) {
  listeners.forEach((listener) => listener());
}

function subscribe(listeners: Set<Listener>, listener: Listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
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

export function publishTranslationPartial(
  subtitleId: number,
  partial: TranslationPartial,
) {
  translationPartials.set(subtitleId, partial);
  notify(translationPartialListeners);
}

export function clearTranslationPartial(subtitleId: number) {
  if (!translationPartials.delete(subtitleId)) return;
  notify(translationPartialListeners);
}

export function clearTranslationPartials() {
  if (!translationPartials.size) return;
  translationPartials.clear();
  notify(translationPartialListeners);
}

export function useTranslationPartial(
  subtitleId: number | null,
): TranslationPartial | null {
  return useSyncExternalStore(
    (listener) => subscribe(translationPartialListeners, listener),
    () => subtitleId === null ? null : translationPartials.get(subtitleId) ?? null,
    () => null,
  );
}
