import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent, RefObject } from "react";

export type SubtitleSelectionRect = {
  top: number;
  left: number;
  width: number;
  height: number;
};

type DragState = {
  pointerId: number;
  startX: number;
  startY: number;
  dragging: boolean;
};

export function useSubtitleBoxSelection(validIds: readonly number[]): {
  containerRef: RefObject<HTMLElement | null>;
  selectedIds: Set<number>;
  dragRect: SubtitleSelectionRect | null;
  selecting: boolean;
  clearSelection: () => void;
  pointerHandlers: {
    onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
    onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void;
    onPointerUp: (event: ReactPointerEvent<HTMLElement>) => void;
    onPointerCancel: (event: ReactPointerEvent<HTMLElement>) => void;
  };
} {
  const containerRef = useRef<HTMLElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const pendingRectRef = useRef<SubtitleSelectionRect | null>(null);
  const frameRef = useRef<number | null>(null);
  const validIdSet = useMemo(() => new Set(validIds), [validIds]);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(() => new Set());
  const [dragRect, setDragRect] = useState<SubtitleSelectionRect | null>(null);
  const [selecting, setSelecting] = useState(false);

  const clearSelection = useCallback(() => setSelectedIds(new Set()), []);

  const selectIntersecting = useCallback((rect: SubtitleSelectionRect) => {
    const next = new Set<number>();
    const container = containerRef.current;
    if (container) {
      for (const article of container.querySelectorAll<HTMLElement>("[data-subtitle-id]")) {
        const id = Number(article.dataset.subtitleId);
        const bubble = article.querySelector<HTMLElement>(".bubble");
        if (validIdSet.has(id) && bubble && selectionTouchesRow(rect, bubble.getBoundingClientRect())) {
          next.add(id);
        }
      }
    }
    setSelectedIds(next);
  }, [validIdSet]);

  const applyPendingRect = useCallback(() => {
    frameRef.current = null;
    const rect = pendingRectRef.current;
    if (!rect) return;
    setDragRect(rect);
    selectIntersecting(rect);
  }, [selectIntersecting]);

  const scheduleRect = useCallback((rect: SubtitleSelectionRect) => {
    pendingRectRef.current = rect;
    if (frameRef.current === null) frameRef.current = window.requestAnimationFrame(applyPendingRect);
  }, [applyPendingRect]);

  const finishDrag = useCallback((event: ReactPointerEvent<HTMLElement>, cancelled: boolean) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (frameRef.current !== null) {
      window.cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    }
    if (!cancelled && drag.dragging && pendingRectRef.current) {
      selectIntersecting(pendingRectRef.current);
    } else if (!cancelled && !drag.dragging) {
      clearSelection();
    }
    pendingRectRef.current = null;
    dragRef.current = null;
    setDragRect(null);
    setSelecting(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }, [clearSelection, selectIntersecting]);

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || !canStartBoxSelection(event.target, event.currentTarget)) return;
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      dragging: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  }, []);

  const onPointerMove = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const deltaX = event.clientX - drag.startX;
    const deltaY = event.clientY - drag.startY;
    if (!drag.dragging && deltaX === 0 && deltaY === 0) return;
    if (!drag.dragging) {
      drag.dragging = true;
      setSelecting(true);
    }
    event.preventDefault();
    scheduleRect(selectionRect(drag.startX, drag.startY, event.clientX, event.clientY));
  }, [scheduleRect]);

  const onPointerUp = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    finishDrag(event, false);
  }, [finishDrag]);

  const onPointerCancel = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    finishDrag(event, true);
  }, [finishDrag]);

  useEffect(() => {
    setSelectedIds((current) => {
      const next = new Set([...current].filter((id) => validIdSet.has(id)));
      return next.size === current.size ? current : next;
    });
  }, [validIdSet]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      dragRef.current = null;
      pendingRectRef.current = null;
      if (frameRef.current !== null) window.cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
      setDragRect(null);
      setSelecting(false);
      clearSelection();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [clearSelection]);

  useEffect(() => () => {
    if (frameRef.current !== null) window.cancelAnimationFrame(frameRef.current);
  }, []);

  return {
    containerRef,
    selectedIds,
    dragRect,
    selecting,
    clearSelection,
    pointerHandlers: { onPointerDown, onPointerMove, onPointerUp, onPointerCancel },
  };
}

function canStartBoxSelection(target: EventTarget, container: HTMLElement): boolean {
  if (!(target instanceof Element) || !container.contains(target)) return false;
  return !target.closest(".bubble, button, a, input, textarea, select, [contenteditable='true'], [role='button']");
}

export function selectionRect(startX: number, startY: number, endX: number, endY: number): SubtitleSelectionRect {
  return {
    top: Math.min(startY, endY),
    left: Math.min(startX, endX),
    width: Math.abs(endX - startX),
    height: Math.abs(endY - startY),
  };
}

function selectionTouchesRow(selection: SubtitleSelectionRect, target: DOMRect): boolean {
  return selection.top < target.bottom
    && selection.top + selection.height > target.top;
}
