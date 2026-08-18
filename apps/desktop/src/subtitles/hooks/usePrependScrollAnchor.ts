import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import type { RefObject } from "react";

import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../../app/interface-scale";
import { prependScrollAdjustment } from "../../shared/lib/prepend-scroll";

type SubtitleAnchor = {
  id: string;
  offset: number;
};

type PendingPrepend = {
  anchor: SubtitleAnchor | null;
  previousScrollHeight: number;
};

function firstVisibleSubtitle(
  scrollRegion: HTMLElement,
  list: HTMLElement,
): SubtitleAnchor | null {
  const viewportTop = scrollRegion.getBoundingClientRect().top;
  const scale = readAppliedInterfaceScaleFactor();
  const rows = list.querySelectorAll<HTMLElement>("[data-subtitle-id]");
  for (const row of rows) {
    const rect = row.getBoundingClientRect();
    if (rect.bottom <= viewportTop) continue;
    const id = row.dataset.subtitleId;
    return id
      ? { id, offset: interfaceLayoutPixels(rect.top - viewportTop, scale) }
      : null;
  }
  return null;
}

function subtitleRow(list: HTMLElement, id: string): HTMLElement | null {
  const rows = list.querySelectorAll<HTMLElement>("[data-subtitle-id]");
  return [...rows].find((row) => row.dataset.subtitleId === id) ?? null;
}


export function usePrependScrollAnchor(
  loading: boolean,
  scrollContainerRef: RefObject<HTMLDivElement | null>,
  listRef: RefObject<HTMLElement | null>,
) {
  const pendingRef = useRef<PendingPrepend | null>(null);
  const clearTimerRef = useRef<number | null>(null);

  const clearTimer = useCallback(() => {
    if (clearTimerRef.current !== null) window.clearTimeout(clearTimerRef.current);
    clearTimerRef.current = null;
  }, []);

  useEffect(() => clearTimer, [clearTimer]);

  useLayoutEffect(() => {
    const pending = pendingRef.current;
    if (!pending || loading) return;

    const scrollRegion = scrollContainerRef.current;
    const list = listRef.current;
    if (scrollRegion && list) {
      const row = pending.anchor ? subtitleRow(list, pending.anchor.id) : null;
      const scale = readAppliedInterfaceScaleFactor();
      const currentOffset = row
        ? interfaceLayoutPixels(
            row.getBoundingClientRect().top - scrollRegion.getBoundingClientRect().top,
            scale,
          )
        : null;
      scrollRegion.scrollTop += prependScrollAdjustment(
        pending.anchor?.offset ?? null,
        currentOffset,
        pending.previousScrollHeight,
        scrollRegion.scrollHeight,
      );
    }

    pendingRef.current = null;
    clearTimer();
  }, [clearTimer, listRef, loading, scrollContainerRef]);

  return useCallback(async (prepend: () => Promise<void>) => {
    const scrollRegion = scrollContainerRef.current;
    const list = listRef.current;
    if (!scrollRegion || !list) {
      await prepend();
      return;
    }

    clearTimer();
    pendingRef.current = {
      anchor: firstVisibleSubtitle(scrollRegion, list),
      previousScrollHeight: scrollRegion.scrollHeight,
    };

    await prepend();
    if (pendingRef.current) {
      clearTimerRef.current = window.setTimeout(() => {
        pendingRef.current = null;
        clearTimerRef.current = null;
      }, 1_000);
    }
  }, [clearTimer, listRef, scrollContainerRef]);
}
