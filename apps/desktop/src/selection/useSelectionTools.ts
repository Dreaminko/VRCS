import { useCallback, useState } from "react";

import type { LookupOrigin } from "../app/app-types";
import type { ReportRuntimeError } from "../core-client/useRuntimeErrors";
import { useDictionaryLookup } from "../dictionary/useDictionaryLookup";
import { useTextSelection } from "./useTextSelection";

export function useSelectionTools({
  compact,
  dictionaryLookupEnabled,
  resizeCompactWindow,
  reportError,
}: {
  compact: boolean;
  dictionaryLookupEnabled: boolean;
  resizeCompactWindow: (expanded: boolean) => Promise<void>;
  reportError: ReportRuntimeError;
}) {
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

  const selectText = useCallback(async (context: string, origin?: LookupOrigin) => {
    setTool(null);
    dictionary.clearLookup();
    const target = await textSelection.captureSelection(context, origin);
    if (!target) return;

    if (!dictionaryLookupEnabled) {
      setTool("ai");
      return;
    }

    setTool("dictionary");
    await dictionary.lookupSelection(target);
  }, [dictionary, dictionaryLookupEnabled, textSelection.captureSelection]);

  const openAi = useCallback(async () => {
    if (compact) {
      try {
        await resizeCompactWindow(true);
      } catch (reason) {
        reportError(reason, "errors.window.compactToggle", "window");
      }
    }
    setTool("ai");
  }, [compact, reportError, resizeCompactWindow]);

  const returnToDictionary = useCallback(() => {
    if (dictionary.lookup) setTool("dictionary");
  }, [dictionary.lookup]);

  return {
    tool,
    target: textSelection.target,
    lookup: dictionary.lookup,
    lookupLoading: dictionary.loading,
    clear,
    close,
    selectText,
    openAi,
    returnToDictionary,
  };
}
