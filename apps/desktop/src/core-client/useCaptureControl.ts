import { useCallback, useState } from "react";

import { shouldShowVrchatNotRunningWarning } from "../capture/capture-warning";
import {
  readTranscriptionStartBehavior,
  shouldCreateConversationOnCaptureToggle,
} from "../capture/transcription-start";

type ReportError = (
  reason: unknown,
  fallbackKey: string,
  source?: string,
) => void;

export function useCaptureControl({
  coreReady,
  running,
  outputMode,
  createConversation,
  clearLookup,
  toggleCoreCapture,
  clearErrorFrom,
  reportError,
}: {
  coreReady: boolean;
  running: boolean;
  outputMode: string | undefined;
  createConversation: () => Promise<boolean>;
  clearLookup: () => void;
  toggleCoreCapture: () => Promise<unknown>;
  clearErrorFrom: (source: string) => void;
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
        clearErrorFrom("capture");
        clearLookup();
        setVrchatWarningOpen(true);
      } else {
        reportError(reason, "errors.operation", "capture");
      }
      return false;
    }
  }, [
    clearErrorFrom,
    clearLookup,
    coreReady,
    createConversation,
    outputMode,
    reportError,
    running,
    toggleCoreCapture,
  ]);

  const closeVrchatWarning = useCallback(() => {
    setVrchatWarningOpen(false);
  }, []);

  return {
    vrchatWarningOpen,
    toggleCapture,
    closeVrchatWarning,
  };
}
