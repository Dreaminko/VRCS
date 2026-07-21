import type { DictionaryEntry, Subtitle } from "./types";

export const COMPACT_WINDOW_SIZE = { width: 720, height: 120 } as const;
export const DICTIONARY_WINDOW_SIZE = { width: 380, height: 400 } as const;
export const DICTIONARY_WINDOW_READY_EVENT = "dictionary-window-ready";
export const DICTIONARY_WINDOW_PAYLOAD_EVENT = "dictionary-window-payload";
export const DICTIONARY_WINDOW_RENDERED_EVENT = "dictionary-window-rendered";
export const DICTIONARY_WINDOW_DESTROYED_EVENT = "tauri://destroyed";

export type DictionaryWindowPayload = {
  term: string;
  context: string;
  entries: DictionaryEntry[];
};

export type DictionaryWindowLifecycle = {
  generation: number;
  cleanup: (() => void) | null;
};

export function createDictionaryWindowLifecycle(): DictionaryWindowLifecycle {
  return { generation: 0, cleanup: null };
}

export function beginDictionaryWindowRequest(lifecycle: DictionaryWindowLifecycle) {
  lifecycle.cleanup?.();
  lifecycle.cleanup = null;
  lifecycle.generation += 1;
  return lifecycle.generation;
}

export function trackDictionaryWindowRequest(
  lifecycle: DictionaryWindowLifecycle,
  generation: number,
  cleanup: () => void,
) {
  if (lifecycle.generation !== generation) {
    cleanup();
    return false;
  }
  lifecycle.cleanup = cleanup;
  return true;
}

export function isCurrentDictionaryWindowRequest(
  lifecycle: DictionaryWindowLifecycle,
  generation: number,
) {
  return lifecycle.generation === generation;
}

export function dictionaryWindowUrl() {
  return "index.html?dictionary-window=1";
}

export function dictionaryWindowOptions(title: string) {
  return {
    url: dictionaryWindowUrl(),
    title,
    ...DICTIONARY_WINDOW_SIZE,
    decorations: false,
    resizable: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    visible: false,
  } as const;
}

export function isDictionaryWindow(search: string) {
  return new URLSearchParams(search).has("dictionary-window");
}

export function observeDictionaryWindowDestroyed(
  window: { once: (event: string, handler: () => void) => Promise<() => void> },
  onDestroyed: () => void,
) {
  return window.once(DICTIONARY_WINDOW_DESTROYED_EVENT, onDestroyed);
}

type WindowPosition = { x: number; y: number };

function samePosition(left: WindowPosition, right: WindowPosition) {
  return Math.abs(left.x - right.x) <= 1 && Math.abs(left.y - right.y) <= 1;
}

export async function prepareDictionaryWindow<T extends WindowPosition>(
  window: {
    setPosition: (position: T) => Promise<void>;
    outerPosition: () => Promise<WindowPosition>;
    onMoved: (handler: (event: { payload: WindowPosition }) => void) => Promise<() => void>;
  },
  position: T,
) {
  let confirmMove: () => void = () => {};
  const moved = new Promise<void>((resolve) => {
    confirmMove = resolve;
  });
  const stopWatching = await window.onMoved(({ payload }) => {
    if (samePosition(payload, position)) confirmMove();
  });
  try {
    await window.setPosition(position);
    if (!samePosition(await window.outerPosition(), position)) await moved;
  } finally {
    stopWatching();
  }
}

export async function revealDictionaryWindow(
  window: { show: () => Promise<void> },
  positionReady: Promise<void>,
) {
  await positionReady;
  await window.show();
}

export function detachedDictionaryPosition({
  anchor,
  windowPosition,
  monitorPosition,
  monitorSize,
  scaleFactor,
}: {
  anchor: { top: number; bottom: number; centerX: number };
  windowPosition: { x: number; y: number };
  monitorPosition: { x: number; y: number };
  monitorSize: { width: number; height: number };
  scaleFactor: number;
}) {
  const gap = 10 * scaleFactor;
  const width = DICTIONARY_WINDOW_SIZE.width * scaleFactor;
  const height = DICTIONARY_WINDOW_SIZE.height * scaleFactor;
  const minX = monitorPosition.x;
  const minY = monitorPosition.y;
  const maxX = minX + monitorSize.width - width;
  const maxY = minY + monitorSize.height - height;
  const below = windowPosition.y + anchor.bottom * scaleFactor + gap;
  const above = windowPosition.y + anchor.top * scaleFactor - gap - height;

  return {
    x: Math.round(Math.min(Math.max(windowPosition.x + anchor.centerX * scaleFactor - width / 2, minX), maxX)),
    y: Math.round(below <= maxY ? below : Math.max(minY, above)),
  };
}

export function subtitleForCompactView(subtitles: Subtitle[], lookupContext?: string) {
  if (!lookupContext) return subtitles[0];
  return subtitles.find((subtitle) => subtitle.text === lookupContext) ?? subtitles[0];
}
