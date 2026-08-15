export const MIN_MICROPHONE_LEVEL_DBFS = -80;
export const MAX_MICROPHONE_THRESHOLD_DBFS = -10;
export const DEFAULT_MICROPHONE_THRESHOLD_DBFS = -45;

export interface MicrophoneCalibrationResult {
  threshold: number;
  noiseLevel: number;
  speechLevel: number;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function percentile(values: number[], ratio: number): number | null {
  const finite = values.filter(Number.isFinite).sort((left, right) => left - right);
  if (!finite.length) return null;
  const index = Math.min(finite.length - 1, Math.max(0, Math.round((finite.length - 1) * ratio)));
  return finite[index];
}

export function suggestMicrophoneThreshold(
  quietSamples: number[],
  speechSamples: number[],
): MicrophoneCalibrationResult | null {
  const noiseLevel = percentile(quietSamples, 0.85);
  const speechLevel = percentile(speechSamples, 0.6);
  if (noiseLevel === null || speechLevel === null || speechLevel - noiseLevel < 5) return null;

  return {
    threshold: Math.round(clamp((noiseLevel + speechLevel) / 2, MIN_MICROPHONE_LEVEL_DBFS, MAX_MICROPHONE_THRESHOLD_DBFS)),
    noiseLevel: Math.round(noiseLevel),
    speechLevel: Math.round(speechLevel),
  };
}
