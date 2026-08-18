import { useCallback, useEffect, useState } from "react";

import {
  completeOnboarding,
  loadOnboardingState,
  needsOnboarding,
  saveOnboardingProgress,
} from "./onboarding-state";

export type OnboardingStatus = "loading" | "required" | "complete";

export function useOnboardingFlow() {
  const [status, setStatus] = useState<OnboardingStatus>("loading");
  const [step, setStep] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void loadOnboardingState().then(
      (state) => {
        if (cancelled) return;
        setStep(state.currentStep);
        setStatus(needsOnboarding(state) ? "required" : "complete");
      },
      () => {
        if (!cancelled) setStatus("required");
      },
    );
    return () => { cancelled = true; };
  }, []);

  const saveProgress = useCallback(async (nextStep: number) => {
    await saveOnboardingProgress(nextStep);
    setStep(nextStep);
  }, []);

  const skip = useCallback(async () => {
    await completeOnboarding();
    setStatus("complete");
  }, []);

  const finish = useCallback(async ({
    startCapture,
    startCaptureAction,
    startCaptureError,
  }: {
    startCapture: boolean;
    startCaptureAction: () => Promise<boolean>;
    startCaptureError: string;
  }) => {
    if (startCapture && !(await startCaptureAction())) {
      throw new Error(startCaptureError);
    }
    await completeOnboarding();
    setStatus("complete");
  }, []);

  const restart = useCallback(() => {
    setStep(0);
    setStatus("required");
  }, []);

  return {
    status,
    step,
    saveProgress,
    skip,
    finish,
    restart,
  };
}
