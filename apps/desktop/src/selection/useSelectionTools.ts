import { useCallback, useEffect, useRef, useState } from "react";

import type { LookupOrigin } from "../app/app-types";
import type { ReportRuntimeError } from "../core-client/useRuntimeErrors";
import { useDictionaryLookup } from "../dictionary/useDictionaryLookup";
import { useTextSelection } from "./useTextSelection";

export function useSelectionTools({
  compact,
  enabled,
  dictionaryLookupEnabled,
  resizeCompactWindow,
  reportError,
}: {
  compact: boolean;
  enabled: boolean;
  dictionaryLookupEnabled: boolean;
  resizeCompactWindow: (expanded: boolean) => Promise<void>;
  reportError: ReportRuntimeError;
}) {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const [tool, setTool] = useState<"dictionary" | "ai" | null>(null);
  const dictionary = useDictionaryLookup({ reportError });
  const textSelection = useTextSelection({ compact, resizeCompactWindow, reportError });

  const clear = useCallback(() => {
    setTool(null);
    dictionary.clearLookup();
    textSelection.clearSelection();
  }, [dictionary.clearLookup, textSelection.clearSelection]);

  const close = useCallback(() => {
    clear();
    if (compact) {
      void resizeCompactWindow(false).catch((reason) => {
        reportError(reason, "errors.window.compactCollapse", "window");
      });
    }
  }, [clear, compact, reportError, resizeCompactWindow]);

  useEffect(() => {
    if (!enabled) close();
  }, [enabled, close]);

  const selectText = useCallback(async (context: string, origin?: LookupOrigin) => {
    if (!enabledRef.current) return;
    setTool(null);
    dictionary.clearLookup();
    const target = await textSelection.captureSelection(context, origin);
    if (!target || !enabledRef.current) return;

    if (!dictionaryLookupEnabled) {
      setTool("ai");
      return;
    }

    setTool("dictionary");
    await dictionary.lookupSelection(target);
  }, [dictionary, dictionaryLookupEnabled, textSelection.captureSelection]);

  const openAi = useCallback(async () => {
    if (!enabledRef.current) return;
    if (compact) {
      try {
        await resizeCompactWindow(true);
      } catch (reason) {
        reportError(reason, "errors.window.compactToggle", "window");
      }
    }
    if (enabledRef.current) setTool("ai");
  }, [compact, reportError, resizeCompactWindow]);

  const returnToDictionary = useCallback(() => {
    if (dictionary.lookup) setTool("dictionary");
  }, [dictionary.lookup]);

  return {
    tool: enabled ? tool : null,
    target: enabled ? textSelection.target : null,
    lookup: dictionary.lookup,
    lookupLoading: dictionary.loading,
    clear,
    close,
    selectText,
    openAi,
    returnToDictionary,
  };
}
