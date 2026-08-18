import { useEffect, useRef, useState } from "react";

import { useAudioLevel } from "../../realtime-state";
import { useMicrophoneCalibration } from "../../settings/hooks/useMicrophoneCalibration";
import type { Health, Settings } from "../../types";

export function useOnboardingMicrophone({
  step,
  settings,
  health,
  updateSettings,
  onStartMicrophoneTest,
  onStopMicrophoneTest,
  clearMessage,
  showError,
}: {
  step: number;
  settings: Settings;
  health: Health | null;
  updateSettings: (update: (current: Settings) => Settings) => Promise<void>;
  onStartMicrophoneTest: () => Promise<void>;
  onStopMicrophoneTest: () => Promise<void>;
  clearMessage: () => void;
  showError: (reason: unknown, fallbackKey?: string) => void;
}) {
  const microphoneLevel = useAudioLevel("microphone");
  const [testBusy, setTestBusy] = useState(false);
  const [reviewed, setReviewed] = useState(settings.audio.microphone.mode === "disabled");
  const startedByWizardRef = useRef(false);
  const calibration = useMicrophoneCalibration({
    level: microphoneLevel,
    testing: health?.microphone_test_running ?? false,
    onStartTest: async () => {
      await onStartMicrophoneTest();
      startedByWizardRef.current = true;
    },
  });
  const operationBusy = testBusy || calibration.calibrating;

  useEffect(() => {
    if (step === 3 || !startedByWizardRef.current) return;
    calibration.reset();
    startedByWizardRef.current = false;
    void onStopMicrophoneTest().catch(() => undefined);
  }, [calibration.reset, onStopMicrophoneTest, step]);

  useEffect(() => () => {
    if (startedByWizardRef.current) {
      void onStopMicrophoneTest().catch(() => undefined);
    }
  }, [onStopMicrophoneTest]);

  const toggleTest = async () => {
    if (operationBusy) return;
    calibration.reset();
    setTestBusy(true);
    clearMessage();
    try {
      if (health?.microphone_test_running) {
        await onStopMicrophoneTest();
        startedByWizardRef.current = false;
      } else {
        await onStartMicrophoneTest();
        startedByWizardRef.current = true;
        setReviewed(true);
      }
    } catch (reason) {
      showError(reason, "errors.audio.microphoneTestFailed");
    } finally {
      setTestBusy(false);
    }
  };

  const startCalibration = async () => {
    if (operationBusy) return;
    clearMessage();
    try {
      await calibration.start();
      setReviewed(true);
    } catch (reason) {
      showError(reason, "errors.audio.microphoneTestFailed");
    }
  };

  const commitThreshold = (threshold: number) => {
    setReviewed(true);
    void updateSettings((current) => ({
      ...current,
      audio: {
        ...current.audio,
        microphone: {
          ...current.audio.microphone,
          trigger_threshold_dbfs: threshold,
        },
      },
    }));
  };

  const restoreDefault = (threshold: number) => {
    setReviewed(true);
    calibration.reset();
    void updateSettings((current) => ({
      ...current,
      audio: {
        ...current.audio,
        microphone: {
          ...current.audio.microphone,
          trigger_threshold_dbfs: threshold,
        },
      },
    }));
  };

  const applySuggestedThreshold = async () => {
    if (!calibration.result) return;
    const threshold = calibration.result.threshold;
    await updateSettings((current) => ({
      ...current,
      audio: {
        ...current.audio,
        microphone: {
          ...current.audio.microphone,
          trigger_threshold_dbfs: threshold,
        },
      },
    }));
    setReviewed(true);
  };

  const stopAndReset = async () => {
    calibration.reset();
    if (startedByWizardRef.current || health?.microphone_test_running) {
      await onStopMicrophoneTest();
      startedByWizardRef.current = false;
    }
  };

  return {
    microphoneLevel,
    reviewed,
    setReviewed,
    calibration,
    operationBusy,
    toggleTest,
    startCalibration,
    commitThreshold,
    restoreDefault,
    applySuggestedThreshold,
    skipCalibration: () => setReviewed(true),
    stopAndReset,
  };
}
