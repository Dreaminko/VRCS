import { useCallback, useState } from "react";

import { shouldShowVrchatNotRunningWarning } from "../capture-warning";
import type { CompactPanelState } from "../compact-mode";
import {
  readTranscriptionStartBehavior,
  shouldCreateConversationOnCaptureToggle,
} from "../transcription-start";

type ReportError = (
  reason: unknown,
  fallbackKey: string,
  source?: string,
) => void;

export function useCaptureControl({
  coreReady,
  running,
  outputMode,
  compact,
  createConversation,
  clearLookup,
  toggleCoreCapture,
  clearError,
  resizeCompactWindow,
  collapseCompactOverlay,
  reportError,
}: {
  coreReady: boolean;
  running: boolean;
  outputMode: string | undefined;
  compact: boolean;
  createConversation: () => Promise<boolean>;
  clearLookup: () => void;
  toggleCoreCapture: () => Promise<unknown>;
  clearError: () => void;
  resizeCompactWindow: (panelState: CompactPanelState) => Promise<unknown>;
  collapseCompactOverlay: () => void;
  reportError: ReportError;
}) {
  const [vrchatWarningOpen, setVrchatWarningOpen] = useState(false);

  const toggleCapture = useCallback(async (): Promise<boolean> => {
    if (!coreReady) return false;
    try {
      if (shouldCreateConversationOnCaptureToggle(
        running,
        readTranscriptionStartBehavior(),
      )) {
        if (!await createConversation()) return false;
        clearLookup();
      }
      await toggleCoreCapture();
      return true;
    } catch (reason) {
      if (shouldShowVrchatNotRunningWarning(reason, outputMode === "vrchat")) {
        clearError();
        clearLookup();
        setVrchatWarningOpen(true);
        if (compact) {
          try {
            await resizeCompactWindow(true);
          } catch (resizeError) {
            reportError(resizeError, "errors.window.warningExpand", "window");
          }
        }
      } else {
        reportError(reason, "errors.operation", "capture");
      }
      return false;
    }
  }, [
    clearError,
    clearLookup,
    compact,
    coreReady,
    createConversation,
    outputMode,
    reportError,
    resizeCompactWindow,
    running,
    toggleCoreCapture,
  ]);

  const closeVrchatWarning = useCallback(() => {
    setVrchatWarningOpen(false);
    if (compact) collapseCompactOverlay();
  }, [collapseCompactOverlay, compact]);

  return {
    vrchatWarningOpen,
    toggleCapture,
    closeVrchatWarning,
  };
}
