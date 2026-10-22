import { useCallback, useRef, useState } from "react";
import { createCaptureController } from "../capture/capture-control";

import { shouldShowVrchatNotRunningWarning } from "../capture/capture-warning";
import type { CompactPanelState } from "../shell/compact-mode";
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
  compact,
  createConversation,
  clearLookup,
  setCoreCaptureRunning,
  clearErrorFrom,
  resizeCompactWindow,
  collapseCompactOverlay,
  reportError,
}: {
  coreReady: boolean;
  getRunning: () => boolean;
  outputMode: string | undefined;
  compact: boolean;
  createConversation: () => Promise<boolean>;
  clearLookup: () => void;
  setCoreCaptureRunning: (target: boolean) => Promise<boolean>;
  clearErrorFrom: (source: string) => void;
  resizeCompactWindow: (panelState: CompactPanelState) => Promise<unknown>;
  collapseCompactOverlay: () => void;
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
    clearErrorFrom,
    clearLookup,
    compact,
    outputMode,
    reportError,
    resizeCompactWindow,
  ]);
  const toggleCapture = useCallback(() => setCaptureRunning(!current.current.getRunning()), [setCaptureRunning]);

  const closeVrchatWarning = useCallback(() => {
    setVrchatWarningOpen(false);
    if (compact) collapseCompactOverlay();
  }, [collapseCompactOverlay, compact]);

  return {
    vrchatWarningOpen,
    toggleCapture,
    setCaptureRunning,
    pending,
    closeVrchatWarning,
  };
}
