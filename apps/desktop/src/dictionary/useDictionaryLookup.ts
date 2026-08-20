import { useCallback, useRef, useState } from "react";

import { coreApi } from "../api";
import type { Lookup, SelectionTarget } from "../app/app-types";

export function useDictionaryLookup({
  reportError,
}: {
  reportError: (reason: unknown, fallbackKey: string) => void;
}) {
  const [lookup, setLookup] = useState<Lookup | null>(null);
  const [loading, setLoading] = useState(false);
  const requestRef = useRef(0);

  const clearLookup = useCallback(() => {
    requestRef.current += 1;
    setLookup(null);
    setLoading(false);
  }, []);

  const lookupSelection = useCallback(async (target: SelectionTarget): Promise<boolean> => {
    const requestId = ++requestRef.current;
    setLookup({
      ...target,
      term: target.selectedText,
      entries: [],
    });
    setLoading(true);
    try {
      const entries = await coreApi.lookup(target.selectedText);
      if (requestId !== requestRef.current) return false;
      setLookup({
        ...target,
        term: target.selectedText,
        entries,
      });
      return true;
    } catch (reason) {
      if (requestId === requestRef.current) {
        reportError(reason, "errors.dictionary.lookup");
      }
      return false;
    } finally {
      if (requestId === requestRef.current) setLoading(false);
    }
  }, [reportError]);

  return {
    lookup,
    loading,
    clearLookup,
    lookupSelection,
  };
}
