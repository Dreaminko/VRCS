import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, Square } from "lucide-react";

import type { AudioLevel } from "../../types";
import { RangeField } from "../SettingsControls";

const MIN_LEVEL_DBFS = -80;
const MAX_LEVEL_DBFS = 0;
const MAX_THRESHOLD_DBFS = -10;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function levelPercent(value: number): number {
  return ((clamp(value, MIN_LEVEL_DBFS, MAX_LEVEL_DBFS) - MIN_LEVEL_DBFS)
    / (MAX_LEVEL_DBFS - MIN_LEVEL_DBFS)) * 100;
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
  const rmsDbfs = active && level ? level.rms_dbfs : MIN_LEVEL_DBFS;
  const peakDbfs = active && level ? level.peak_dbfs : MIN_LEVEL_DBFS;
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
    "--microphone-threshold": `${levelPercent(threshold)}%`,
  } as CSSProperties;
  const formatDbfs = (value: number) => t("units.dbfs", { value: Math.round(value) });

  return (
    <div className={`microphone-level-setting ${speech ? "triggered" : ""}`}>
      <div className="microphone-level-header">
        <strong>{t("settings.audio.inputLevel")}</strong>
        <span className="microphone-level-status">
          <span aria-hidden="true" />
          {status}
          {active && <output>{formatDbfs(rmsDbfs)}</output>}
        </span>
      </div>
      <div
        className="microphone-level-meter"
        role="meter"
        aria-label={t("settings.audio.levelMeterLabel")}
        aria-valuemin={MIN_LEVEL_DBFS}
        aria-valuemax={MAX_LEVEL_DBFS}
        aria-valuenow={Math.round(rmsDbfs)}
        aria-valuetext={active ? formatDbfs(rmsDbfs) : status}
        style={meterStyle}
      >
        <span className="microphone-level-fill" aria-hidden="true" />
        <span className="microphone-level-peak" aria-hidden="true" />
        <span
          className="microphone-threshold-marker"
          title={t("settings.audio.thresholdMarkerLabel", { value: Math.round(threshold) })}
          aria-hidden="true"
        />
      </div>
      <div className="microphone-meter-scale" aria-hidden="true">
        <span>-80 dBFS</span>
        <span>0 dBFS</span>
      </div>
      <div className="microphone-test-control">
        <span>{t("settings.audio.microphoneTestDescription")}</span>
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
        helper={t("settings.audio.triggerThresholdDescription")}
        value={threshold}
        min={MIN_LEVEL_DBFS}
        max={MAX_THRESHOLD_DBFS}
        step={1}
        disabled={disabled}
        formatValue={formatDbfs}
        onCommit={onCommit}
      />
    </div>
  );
}
