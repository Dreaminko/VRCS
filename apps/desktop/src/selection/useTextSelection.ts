import { useCallback, useState } from "react";

import type { LookupOrigin, SelectionTarget } from "../app/app-types";
import type { CompactPanelState } from "../compact-mode";

const SELECTION_BOUNDARY_PUNCTUATION = (
  /^[\s.,!?;:，。！？；：“”'"「」『』（）()]+|[\s.,!?;:，。！？；：“”'"「」『』（）()]+$/g
);

export function useTextSelection({
  compact,
  resizeCompactWindow,
  reportError,
}: {
  compact: boolean;
  resizeCompactWindow: (panelState: CompactPanelState) => Promise<void>;
  reportError: (reason: unknown, fallbackKey: string) => void;
}) {
  const [target, setTarget] = useState<SelectionTarget | null>(null);

  const captureSelection = useCallback(async (
    context: string,
    origin?: LookupOrigin,
  ): Promise<SelectionTarget | null> => {
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

    if (compact) {
      try {
        await resizeCompactWindow(true);
      } catch (reason) {
        reportError(reason, "errors.window.compactToggle");
      }
    }
    return next;
  }, [compact, reportError, resizeCompactWindow]);

  const clearSelection = useCallback(() => {
    setTarget(null);
    window.getSelection()?.removeAllRanges();
  }, []);

  return { target, captureSelection, clearSelection };
}
