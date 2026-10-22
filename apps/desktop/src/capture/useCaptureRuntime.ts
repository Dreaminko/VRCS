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
  const { getCurrent, patch, refreshQuietly } = health;

  const setRunning = useCallback(async (target: boolean): Promise<boolean> => {
    if (pendingRef.current) return false;
    if (getCurrent()?.capture_requested === target) return true;
    pendingRef.current = true;
    setPending(true);
    try {
      if (!target) {
        await captureApi.stop();
        clearPartials();
      } else {
        await captureApi.start();
      }
      clearErrorFrom("capture");
      patch((current) => current ? { ...current, capture_requested: target } : current);
      return true;
    } catch (error) {
      // A timeout can leave the operation completed on Core. Reconcile before retrying.
      if ((await refreshQuietly())?.capture_requested === target) return true;
      throw error;
    } finally {
      await refreshQuietly();
      pendingRef.current = false;
      setPending(false);
    }
  }, [clearErrorFrom, clearPartials, getCurrent, patch, refreshQuietly]);

  const toggle = useCallback(() => setRunning(!getCurrent()?.capture_requested), [getCurrent, setRunning]);

  const startMicrophoneTest = useCallback(async () => {
    try {
      const result = await captureApi.startMicrophoneTest();
      patch((current) => current ? {
        ...current,
        microphone_test_running: result.running,
        microphone_test_device: result.device,
      } : current);
      clearErrorFrom("microphone-test");
      void refreshQuietly();
    } catch (reason) {
      reportError(reason, "errors.audio.microphoneTestFailed", "microphone-test");
      throw reason;
    }
  }, [clearErrorFrom, patch, refreshQuietly, reportError]);

  const stopMicrophoneTest = useCallback(async () => {
    try {
      const result = await captureApi.stopMicrophoneTest();
      clearPartials();
      patch((current) => current ? {
        ...current,
        microphone_test_running: result.running,
        microphone_test_device: null,
      } : current);
      clearErrorFrom("microphone-test");
      void refreshQuietly();
    } catch (reason) {
      reportError(reason, "errors.audio.microphoneTestFailed", "microphone-test");
      throw reason;
    }
  }, [clearErrorFrom, clearPartials, patch, refreshQuietly, reportError]);

  return {
    pending,
    toggle,
    setRunning,
    getRunning: () => getCurrent()?.capture_requested ?? false,
    startMicrophoneTest,
    stopMicrophoneTest,
  };
}
