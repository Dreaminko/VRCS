import { useCallback, useRef, useState } from "react";
import { createCaptureController } from "../capture/capture-control";

import { shouldShowVrchatNotRunningWarning } from "../capture/capture-warning";
import {
  readTranscriptionStartBehavior,
} from "../capture/transcription-start";

type ReportError = (
  reason: unknown,
  fallbackKey: string,
  source?: string,
) => void;

export function useCaptureControl({
  coreReady,
  getRunning,
  outputMode,
  createConversation,
  clearLookup,
  setCoreCaptureRunning,
  clearErrorFrom,
  reportError,
}: {
  coreReady: boolean;
  getRunning: () => boolean;
  outputMode: string | undefined;
  createConversation: () => Promise<boolean>;
  clearLookup: () => void;
  setCoreCaptureRunning: (target: boolean) => Promise<boolean>;
  clearErrorFrom: (source: string) => void;
  reportError: ReportError;
}) {
  const [vrchatWarningOpen, setVrchatWarningOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const current = useRef({ coreReady, getRunning, createConversation, clearLookup, setCoreCaptureRunning });
  current.current = { coreReady, getRunning, createConversation, clearLookup, setCoreCaptureRunning };
  const controller = useRef<ReturnType<typeof createCaptureController> | null>(null);
  if (!controller.current) controller.current = createCaptureController({
    available: () => current.current.coreReady,
    running: () => current.current.getRunning(),
    newConversation: () => readTranscriptionStartBehavior() === "new_conversation",
    createConversation: async () => {
      if (!await current.current.createConversation()) return false;
      current.current.clearLookup();
      return true;
    },
    setRunning: (target) => current.current.setCoreCaptureRunning(target),
    onPending: setPending,
  });

  const setCaptureRunning = useCallback(async (target: boolean): Promise<boolean> => {
    try {
      return await controller.current!.setRunning(target);
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
    outputMode,
    reportError,
  ]);
  const toggleCapture = useCallback(() => setCaptureRunning(!current.current.getRunning()), [setCaptureRunning]);

  const closeVrchatWarning = useCallback(() => {
    setVrchatWarningOpen(false);
  }, []);

  return {
    vrchatWarningOpen,
    toggleCapture,
    setCaptureRunning,
    pending,
    closeVrchatWarning,
  };
}
