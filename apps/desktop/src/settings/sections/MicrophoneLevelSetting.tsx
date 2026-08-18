import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, Square } from "lucide-react";

import {
  MAX_MICROPHONE_THRESHOLD_DBFS,
  MIN_MICROPHONE_LEVEL_DBFS,
} from "../../microphone-calibration";
import type { AudioLevel } from "../../types";
import { RangeField } from "../SettingsControls";

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

// Use the threshold slider scale so its marker aligns with the level meter.
function levelPercent(value: number): number {
  return ((clamp(value, MIN_MICROPHONE_LEVEL_DBFS, MAX_MICROPHONE_THRESHOLD_DBFS) - MIN_MICROPHONE_LEVEL_DBFS)
    / (MAX_MICROPHONE_THRESHOLD_DBFS - MIN_MICROPHONE_LEVEL_DBFS)) * 100;
}

export function MicrophoneLevelSetting({
  level,
  enabled,
  captureRunning,
  transcriptionRunning,
  testing,
  busy,
  threshold,
  disabled,
  onToggleTest,
  onCommit,
}: {
  level: AudioLevel | null;
  enabled: boolean;
  captureRunning: boolean;
  transcriptionRunning: boolean;
  testing: boolean;
  busy: boolean;
  threshold: number;
  disabled: boolean;
  onToggleTest: () => void;
  onCommit: (value: number) => void;
}) {
  const { t } = useTranslation();
  const active = captureRunning || testing;
  const rmsDbfs = active && level ? level.rms_dbfs : MIN_MICROPHONE_LEVEL_DBFS;
  const peakDbfs = active && level ? level.peak_dbfs : MIN_MICROPHONE_LEVEL_DBFS;
  const speech = Boolean(
    active && level && (testing ? level.rms_dbfs >= threshold : level.speech),
  );
  const status = !enabled
    ? t("settings.audio.previewDisabled")
    : testing && speech
      ? t("settings.audio.previewThresholdReached")
      : testing
        ? t("settings.audio.previewTesting")
        : captureRunning
          ? t("settings.audio.previewTranscribing")
          : t("settings.audio.previewReady");
  const meterStyle = {
    "--microphone-level-scale": levelPercent(rmsDbfs) / 100,
    "--microphone-peak": `${levelPercent(peakDbfs)}%`,
  } as CSSProperties;
  const formatDbfs = (value: number) => t("units.dbfs", { value: Math.round(value) });

  return (
    <div className={`microphone-level-setting ${speech ? "triggered" : ""}`}>
      <div className="microphone-test-control">
        <span className="microphone-level-status">
          {status}
        </span>
        <button
          className="secondary-button microphone-test-button"
          type="button"
          aria-pressed={testing}
          disabled={disabled || !enabled || transcriptionRunning || busy}
          onClick={onToggleTest}
        >
          {testing ? <Square size={14} /> : <AudioLines size={16} />}
          {busy
            ? t("settings.audio.microphoneTestBusy")
            : testing
              ? t("settings.audio.stopMicrophoneTest")
              : t("settings.audio.startMicrophoneTest")}
        </button>
      </div>
      <RangeField
        label={t("settings.audio.triggerThreshold")}
        value={threshold}
        min={MIN_MICROPHONE_LEVEL_DBFS}
        max={MAX_MICROPHONE_THRESHOLD_DBFS}
        step={1}
        disabled={disabled}
        formatValue={formatDbfs}
        hideValue
        hideBounds
        trackSlot={(
          <span
            className="microphone-live-track"
            role="meter"
            aria-label={t("settings.audio.levelMeterLabel")}
            aria-valuemin={MIN_MICROPHONE_LEVEL_DBFS}
            aria-valuemax={MAX_MICROPHONE_THRESHOLD_DBFS}
            aria-valuenow={Math.round(rmsDbfs)}
            aria-valuetext={status}
            style={meterStyle}
          >
            <span className="microphone-live-fill" aria-hidden="true" />
            <span className="microphone-live-peak" aria-hidden="true" />
          </span>
        )}
        onCommit={onCommit}
      />
    </div>
  );
}
