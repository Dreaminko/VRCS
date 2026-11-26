import { useLayoutEffect, useMemo, useRef, useState } from "react";

export const PREVIEW_INTERVAL_MS = 50;

interface PreviewTimers {
  schedule(callback: () => void, delay: number): unknown;
  cancel(timer: unknown): void;
}

const previewTimers: PreviewTimers = {
  schedule: (callback, delay) => setTimeout(callback, delay),
  cancel: (timer) => clearTimeout(timer as ReturnType<typeof setTimeout>),
};

export function createPreviewBatcher<T>(publish: (value: T) => void, timers = previewTimers) {
  let timer: unknown;
  let pending: { key: string; value: T } | undefined;
  const cancel = () => {
    if (pending) timers.cancel(timer);
    pending = undefined;
  };
  return {
    cancel,
    push(value: T, key: string) {
      if (pending?.key === key) {
        pending.value = value;
        return;
      }
      cancel();
      pending = { key, value };
      // New deltas replace the snapshot without extending the current window.
      timer = timers.schedule(() => {
        const next = pending;
        pending = undefined;
        if (next) publish(next.value);
      }, PREVIEW_INTERVAL_MS);
    },
  };
}

// Only display previews throttle rapid updates; initial frames and final results stay immediate.
export function useBatchedPreview<T>(value: T | null, key: string, enabled: boolean): T | null {
  const [display, setDisplay] = useState<{ key: string; value: T } | null>(null);
  const displayedKeyRef = useRef<string | null>(null);
  const batcher = useMemo(() => createPreviewBatcher<{ key: string; value: T }>((next) => {
    displayedKeyRef.current = next.key;
    setDisplay(next);
  }), []);
  useLayoutEffect(() => {
    if (!enabled || value === null) {
      batcher.cancel();
      displayedKeyRef.current = null;
      setDisplay(null);
      return;
    }
    if (displayedKeyRef.current !== key) {
      batcher.cancel();
      displayedKeyRef.current = key;
      setDisplay({ key, value });
      return;
    }
    batcher.push({ key, value }, key);
  }, [batcher, enabled, key, value]);
  useLayoutEffect(() => () => batcher.cancel(), [batcher]);
  if (!enabled || value === null) return value;
  return display?.key === key ? display.value : value;
}
