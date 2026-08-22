import { useCallback, useRef, useState } from "react";

import type { CoreHealthController } from "../core-client/useCoreRuntime";
import type { ReportRuntimeError } from "../core-client/useRuntimeErrors";
import { captureApi } from "./api";

export function useCaptureRuntime({
  health,
  clearPartials,
  clearErrorFrom,
  reportError,
}: {
  health: CoreHealthController;
  clearPartials: () => void;
  clearErrorFrom: (source: string) => void;
  reportError: ReportRuntimeError;
}) {
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);

  const toggle = useCallback(async () => {
    if (pendingRef.current) return;
    pendingRef.current = true;
    setPending(true);
    try {
      if (health.getCurrent()?.capture_requested) {
        await captureApi.stop();
        clearPartials();
      } else {
        await captureApi.start();
      }
      clearErrorFrom("capture");
    } finally {
      await health.refreshQuietly();
      pendingRef.current = false;
      setPending(false);
    }
  }, [clearErrorFrom, clearPartials, health]);

  const startMicrophoneTest = useCallback(async () => {
    try {
      const result = await captureApi.startMicrophoneTest();
      health.patch((current) => current ? {
        ...current,
        microphone_test_running: result.running,
        microphone_test_device: result.device,
      } : current);
      clearErrorFrom("microphone-test");
      void health.refreshQuietly();
    } catch (reason) {
      reportError(reason, "errors.audio.microphoneTestFailed", "microphone-test");
      throw reason;
    }
  }, [clearErrorFrom, health, reportError]);

  const stopMicrophoneTest = useCallback(async () => {
    try {
      const result = await captureApi.stopMicrophoneTest();
      clearPartials();
      health.patch((current) => current ? {
        ...current,
        microphone_test_running: result.running,
        microphone_test_device: null,
      } : current);
      clearErrorFrom("microphone-test");
      void health.refreshQuietly();
    } catch (reason) {
      reportError(reason, "errors.audio.microphoneTestFailed", "microphone-test");
      throw reason;
    }
  }, [clearErrorFrom, clearPartials, health, reportError]);

  return {
    pending,
    toggle,
    startMicrophoneTest,
    stopMicrophoneTest,
  };
}
