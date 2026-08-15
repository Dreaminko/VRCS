import { useCallback, useEffect, useRef, useState } from "react";

import {
  suggestMicrophoneThreshold,
  type MicrophoneCalibrationResult,
} from "../microphone-calibration";
import type { AudioLevel } from "../types";

const QUIET_PROMPT_LEAD_MS = 500;
const QUIET_SAMPLE_MS = 2_000;
const SPEECH_PROMPT_LEAD_MS = 800;
const SPEECH_SAMPLE_MS = 4_000;

type MicrophoneCalibrationCollectionPhase = "quiet" | "speech";

export type MicrophoneCalibrationPhase = "idle" | "quiet" | "speech" | "ready" | "failed";

function wait(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

export function useMicrophoneCalibration({
  level,
  testing,
  onStartTest,
}: {
  level: AudioLevel | null;
  testing: boolean;
  onStartTest: () => Promise<void>;
}) {
  const [phase, setPhase] = useState<MicrophoneCalibrationPhase>("idle");
  const [result, setResult] = useState<MicrophoneCalibrationResult | null>(null);
  const [calibrating, setCalibrating] = useState(false);
  const quietSamplesRef = useRef<number[]>([]);
  const speechSamplesRef = useRef<number[]>([]);
  const collectionPhaseRef = useRef<MicrophoneCalibrationCollectionPhase | null>(null);
  const runRef = useRef(0);
  const calibratingRef = useRef(false);

  useEffect(() => {
    if (!level || !testing) return;
    if (collectionPhaseRef.current === "quiet") quietSamplesRef.current.push(level.rms_dbfs);
    if (collectionPhaseRef.current === "speech") speechSamplesRef.current.push(level.rms_dbfs);
  }, [level, testing]);

  useEffect(() => () => {
    runRef.current += 1;
    calibratingRef.current = false;
    collectionPhaseRef.current = null;
  }, []);

  const reset = useCallback(() => {
    runRef.current += 1;
    calibratingRef.current = false;
    collectionPhaseRef.current = null;
    setCalibrating(false);
    setPhase("idle");
    setResult(null);
  }, []);

  const start = useCallback(async () => {
    if (calibratingRef.current) return null;

    const run = ++runRef.current;
    calibratingRef.current = true;
    quietSamplesRef.current = [];
    speechSamplesRef.current = [];
    setResult(null);
    setCalibrating(true);

    try {
      if (!testing) await onStartTest();
      if (run !== runRef.current) return null;

      setPhase("quiet");
      await wait(QUIET_PROMPT_LEAD_MS);
      if (run !== runRef.current) return null;
      collectionPhaseRef.current = "quiet";
      await wait(QUIET_SAMPLE_MS);
      collectionPhaseRef.current = null;
      if (run !== runRef.current) return null;

      setPhase("speech");
      await wait(SPEECH_PROMPT_LEAD_MS);
      if (run !== runRef.current) return null;
      collectionPhaseRef.current = "speech";
      await wait(SPEECH_SAMPLE_MS);
      collectionPhaseRef.current = null;
      if (run !== runRef.current) return null;

      const suggestion = suggestMicrophoneThreshold(
        quietSamplesRef.current,
        speechSamplesRef.current,
      );
      setResult(suggestion);
      setPhase(suggestion ? "ready" : "failed");
      return suggestion;
    } catch (reason) {
      if (run === runRef.current) setPhase("failed");
      throw reason;
    } finally {
      if (run === runRef.current) {
        calibratingRef.current = false;
        collectionPhaseRef.current = null;
        setCalibrating(false);
      }
    }
  }, [onStartTest, testing]);

  return {
    phase,
    result,
    calibrating,
    reset,
    start,
  };
}
