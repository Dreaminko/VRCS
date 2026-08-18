import { useEffect, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";

import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../app/interface-scale";
import {
  MAX_CONVERSATION_SIDEBAR_WIDTH,
  MIN_CONVERSATION_SIDEBAR_WIDTH,
  normalizeConversationSidebarWidth,
} from "./conversation-sidebar-width";

export function useConversationSidebarResize({
  width,
  onWidthChange,
  onResizeStateChange,
  onBeforeStart,
}: {
  width: number;
  onWidthChange: (width: number) => void;
  onResizeStateChange: (resizing: boolean) => void;
  onBeforeStart: () => void;
}) {
  const [resizing, setResizing] = useState(false);
  const resizeStartRef = useRef<{
    pointerId: number;
    pointerX: number;
    width: number;
  } | null>(null);

  useEffect(() => () => onResizeStateChange(false), [onResizeStateChange]);

  const finish = () => {
    resizeStartRef.current = null;
    setResizing(false);
    onResizeStateChange(false);
  };

  const start = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!event.isPrimary || event.button !== 0) return;
    event.preventDefault();
    onBeforeStart();
    const scale = readAppliedInterfaceScaleFactor();
    resizeStartRef.current = {
      pointerId: event.pointerId,
      pointerX: interfaceLayoutPixels(event.clientX, scale),
      width,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    setResizing(true);
    onResizeStateChange(true);
  };

  const resize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const initial = resizeStartRef.current;
    if (!initial || initial.pointerId !== event.pointerId) return;
    event.preventDefault();
    const pointerX = interfaceLayoutPixels(
      event.clientX,
      readAppliedInterfaceScaleFactor(),
    );
    onWidthChange(normalizeConversationSidebarWidth(
      initial.width + pointerX - initial.pointerX,
    ));
  };

  const stop = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (resizeStartRef.current?.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    finish();
  };

  const resizeWithKeyboard = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 32 : 8;
    let nextWidth = width;
    if (event.key === "ArrowLeft") nextWidth -= step;
    else if (event.key === "ArrowRight") nextWidth += step;
    else if (event.key === "Home") nextWidth = MIN_CONVERSATION_SIDEBAR_WIDTH;
    else if (event.key === "End") nextWidth = MAX_CONVERSATION_SIDEBAR_WIDTH;
    else return;
    event.preventDefault();
    onWidthChange(normalizeConversationSidebarWidth(nextWidth));
  };

  return {
    resizing,
    finish,
    start,
    resize,
    stop,
    resizeWithKeyboard,
  };
}
