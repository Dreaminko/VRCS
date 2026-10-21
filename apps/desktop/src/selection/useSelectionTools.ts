import { useCallback, useEffect, useRef, useState } from "react";

import type { LookupOrigin } from "../app/app-types";
import type { ReportRuntimeError } from "../core-client/useRuntimeErrors";
import { useDictionaryLookup } from "../dictionary/useDictionaryLookup";
import { useTextSelection } from "./useTextSelection";

export function useSelectionTools({
  enabled,
  dictionaryLookupEnabled,
  reportError,
}: {
  enabled: boolean;
  dictionaryLookupEnabled: boolean;
  reportError: ReportRuntimeError;
}) {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const [tool, setTool] = useState<"dictionary" | "ai" | null>(null);
  const dictionary = useDictionaryLookup({ reportError });
  const textSelection = useTextSelection();

  const clear = useCallback(() => {
    setTool(null);
    dictionary.clearLookup();
    textSelection.clearSelection();
  }, [dictionary.clearLookup, textSelection.clearSelection]);

  useEffect(() => {
    if (!enabled) clear();
  }, [enabled, clear]);

  const selectText = useCallback(async (context: string, origin?: LookupOrigin) => {
    if (!enabledRef.current) return;
    const target = textSelection.captureSelection(context, origin);
    if (!target) {
      clear();
      return;
    }

    dictionary.clearLookup();
    setTool(dictionaryLookupEnabled ? "dictionary" : "ai");
    if (dictionaryLookupEnabled) await dictionary.lookupSelection(target);
  }, [clear, dictionary, dictionaryLookupEnabled, textSelection.captureSelection]);

  const openAi = useCallback(async () => {
    if (!enabledRef.current) return;
    setTool("ai");
  }, []);

  const returnToDictionary = useCallback(() => {
    if (dictionary.lookup) setTool("dictionary");
  }, [dictionary.lookup]);

  return {
    tool: enabled ? tool : null,
    target: enabled ? textSelection.target : null,
    lookup: dictionary.lookup,
    lookupLoading: dictionary.loading,
    clear,
    close: clear,
    selectText,
    openAi,
    returnToDictionary,
  };
}
