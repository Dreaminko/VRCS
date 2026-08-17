import { useCallback, useEffect, useState } from "react";

import { coreApi } from "../api";
import type { Lookup, LookupOrigin } from "../app-types";

const TERM_BOUNDARY_PUNCTUATION = (
  /^[\s.,!?;:，。！？；：“”'"「」『』（）()]+|[\s.,!?;:，。！？；：“”'"「」『』（）()]+$/g
);

export function useDictionaryLookup({
  enabled,
  compact,
  resizeCompactWindow,
  reportError,
}: {
  enabled: boolean;
  compact: boolean;
  resizeCompactWindow: (lookupOpen: boolean) => Promise<void>;
  reportError: (reason: unknown, fallbackKey: string) => void;
}) {
  const [lookup, setLookup] = useState<Lookup | null>(null);

  const clearLookup = useCallback(() => {
    setLookup(null);
  }, []);

  const closeCompactLookup = useCallback(() => {
    clearLookup();
    void resizeCompactWindow(false).catch((reason) => {
      reportError(reason, "errors.window.compactCollapse");
    });
  }, [clearLookup, reportError, resizeCompactWindow]);

  useEffect(() => {
    if (enabled || !lookup) return;
    setLookup(null);
    if (compact) {
      void resizeCompactWindow(false).catch((reason) => {
        reportError(reason, "errors.window.compactCollapse");
      });
    }
  }, [compact, enabled, lookup, reportError, resizeCompactWindow]);

  const selectWord = useCallback(async (context: string, origin?: LookupOrigin) => {
    if (!enabled) return;
    const selection = window.getSelection();
    const term = selection
      ?.toString()
      .trim()
      .replace(TERM_BOUNDARY_PUNCTUATION, "");
    if (!selection || !term || selection.rangeCount === 0) return;
    const range = selection.getRangeAt(0).cloneRange();
    const rect = range.getBoundingClientRect();
    if (!rect.width && !rect.height) return;
    try {
      const entries = await coreApi.lookup(term);
      setLookup({
        term,
        context,
        entries,
        origin,
        anchor: {
          top: rect.top,
          bottom: rect.bottom,
          centerX: rect.left + rect.width / 2,
        },
        range,
      });
      if (compact) await resizeCompactWindow(true);
    } catch (reason) {
      reportError(reason, "errors.dictionary.lookup");
    }
  }, [compact, enabled, reportError, resizeCompactWindow]);

  return {
    lookup,
    clearLookup,
    closeCompactLookup,
    selectWord,
  };
}
