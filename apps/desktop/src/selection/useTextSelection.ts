import { useCallback, useState } from "react";

import type { LookupOrigin, SelectionTarget } from "../app/app-types";

const SELECTION_BOUNDARY_PUNCTUATION = (
  /^[\s.,!?;:，。！？；：“”'"「」『』（）()]+|[\s.,!?;:，。！？；：“”'"「」『』（）()]+$/g
);

export function useTextSelection() {
  const [target, setTarget] = useState<SelectionTarget | null>(null);

  const captureSelection = useCallback((
    context: string,
    origin?: LookupOrigin,
  ): SelectionTarget | null => {
    const selection = window.getSelection();
    const selectedText = selection
      ?.toString()
      .trim()
      .replace(SELECTION_BOUNDARY_PUNCTUATION, "");
    if (!selection || !selectedText || selection.rangeCount === 0) return null;

    const range = selection.getRangeAt(0).cloneRange();
    const rect = range.getBoundingClientRect();
    if (!rect.width && !rect.height) return null;

    const next = {
      selectedText,
      context,
      origin,
      range,
      anchor: {
        top: rect.top,
        bottom: rect.bottom,
        centerX: rect.left + rect.width / 2,
      },
    } satisfies SelectionTarget;
    setTarget(next);

    return next;
  }, []);

  const clearSelection = useCallback(() => {
    setTarget(null);
    window.getSelection()?.removeAllRanges();
  }, []);

  return { target, captureSelection, clearSelection };
}
