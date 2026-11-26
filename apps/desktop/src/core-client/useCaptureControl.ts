import { useCallback, useRef, useState } from "react";
import { createCaptureController } from "../capture/capture-control";

import { shouldShowVrchatNotRunningWarning } from "../capture/capture-warning";
import {
  readTranscriptionStartBehavior,
} from "../capture/transcription-start";
import type { CaptureResumeAction } from "../capture/usePlayerAudioStart";

type ReportError = (
  reason: unknown,
  fallbackKey: string,
  source?: string,
) => void;

export function useCaptureControl({
  coreReady,
  unavailableReason,
  networkUnavailableReason,
  getRunning,
  getNetworkRunning,
  outputMode,
  networkCaptureEnabled = outputMode === "vrchat_players",
  preparePlayerAudioStart,
  createConversation,
  clearLookup,
  setCoreCaptureRunning,
  setCoreNetworkRunning,
  clearErrorFrom,
  reportError,
}: {
  coreReady: boolean;
  unavailableReason?: string | null;
  networkUnavailableReason?: string | null;
  getRunning: () => boolean;
  getNetworkRunning: () => boolean;
  outputMode: string | undefined;
  networkCaptureEnabled?: boolean;
  preparePlayerAudioStart: (action?: CaptureResumeAction) => Promise<boolean>;
  createConversation: () => Promise<boolean>;
  clearLookup: () => void;
  setCoreCaptureRunning: (target: boolean) => Promise<boolean>;
  setCoreNetworkRunning: (target: boolean) => Promise<boolean>;
  clearErrorFrom: (source: string) => void;
  reportError: ReportError;
}) {
  const [vrchatWarningOpen, setVrchatWarningOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [networkPending, setNetworkPending] = useState(false);
  const current = useRef({
    coreReady,
    getRunning,
    getNetworkRunning,
    createConversation,
    clearLookup,
    setCoreCaptureRunning,
    setCoreNetworkRunning,
    networkCaptureEnabled,
    preparePlayerAudioStart,
  });
  current.current = {
    coreReady,
    getRunning,
    getNetworkRunning,
    createConversation,
    clearLookup,
    setCoreCaptureRunning,
    setCoreNetworkRunning,
    networkCaptureEnabled,
    preparePlayerAudioStart,
  };
  const controller = useRef<ReturnType<typeof createCaptureController> | null>(null);
  if (!controller.current) controller.current = createCaptureController({
    available: () => current.current.coreReady,
    running: () => current.current.getRunning(),
    newConversation: () => readTranscriptionStartBehavior() === "new_conversation",
    beforeStart: () => (
      current.current.networkCaptureEnabled && !current.current.getNetworkRunning()
        ? current.current.preparePlayerAudioStart("start_capture")
        : Promise.resolve(true)
    ),
    createConversation: async () => {
      if (!await current.current.createConversation()) return false;
      current.current.clearLookup();
      return true;
    },
    setRunning: (target) => current.current.setCoreCaptureRunning(target),
    onPending: setPending,
  });

  const networkController = useRef<ReturnType<typeof createCaptureController> | null>(null);
  if (!networkController.current) networkController.current = createCaptureController({
    available: () => current.current.coreReady,
    running: () => current.current.getNetworkRunning(),
    newConversation: () => false,
    createConversation: async () => true,
    beforeStart: () => current.current.preparePlayerAudioStart("start_network_capture"),
    setRunning: (target) => current.current.setCoreNetworkRunning(target),
    onPending: setNetworkPending,
  });

  const setCaptureRunning = useCallback(async (target: boolean): Promise<boolean> => {
    try {
      if (target && unavailableReason) throw { code: unavailableReason };
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
    unavailableReason,
    reportError,
  ]);
  const toggleCapture = useCallback(() => setCaptureRunning(!current.current.getRunning()), [setCaptureRunning]);

  const setNetworkCaptureRunning = useCallback(async (target: boolean): Promise<boolean> => {
    try {
      if (target && networkUnavailableReason) throw { code: networkUnavailableReason };
      return await networkController.current!.setRunning(target);
    } catch (reason) {
      reportError(reason, "errors.operation", "capture");
      return false;
    }
  }, [networkUnavailableReason, reportError]);
  const toggleNetworkCapture = useCallback(
    () => setNetworkCaptureRunning(!current.current.getNetworkRunning()),
    [setNetworkCaptureRunning],
  );

  const closeVrchatWarning = useCallback(() => {
    setVrchatWarningOpen(false);
  }, []);

  return {
    vrchatWarningOpen,
    toggleCapture,
    setCaptureRunning,
    toggleNetworkCapture,
    setNetworkCaptureRunning,
    pending,
    networkPending,
    closeVrchatWarning,
  };
}
