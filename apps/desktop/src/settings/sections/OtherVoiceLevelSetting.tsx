import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";

import {
  MAX_MICROPHONE_THRESHOLD_DBFS,
  MIN_MICROPHONE_LEVEL_DBFS,
} from "../../microphone-calibration";
import type { AudioLevel } from "../../capture/types";
import { RangeField } from "../SettingsControls";

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function levelPercent(value: number): number {
  return ((clamp(value, MIN_MICROPHONE_LEVEL_DBFS, MAX_MICROPHONE_THRESHOLD_DBFS) - MIN_MICROPHONE_LEVEL_DBFS)
    / (MAX_MICROPHONE_THRESHOLD_DBFS - MIN_MICROPHONE_LEVEL_DBFS)) * 100;
}

export function OtherVoiceLevelSetting({
  level,
  enabled,
  captureRunning,
  threshold,
  disabled,
  onCommit,
}: {
  level: AudioLevel | null;
  enabled: boolean;
  captureRunning: boolean;
  threshold: number;
  disabled: boolean;
  onCommit: (value: number) => void;
}) {
  const { t } = useTranslation();
  const active = enabled && captureRunning;
  const rmsDbfs = active && level ? level.rms_dbfs : MIN_MICROPHONE_LEVEL_DBFS;
  const peakDbfs = active && level ? level.peak_dbfs : MIN_MICROPHONE_LEVEL_DBFS;
  const speech = Boolean(active && level?.speech);
  const status = !enabled
    ? t("settings.audio.otherVoicePreviewDisabled")
    : !captureRunning
      ? t("settings.audio.otherVoicePreviewReady")
      : speech
        ? t("settings.audio.previewThresholdReached")
        : t("settings.audio.otherVoicePreviewListening");
  const meterStyle = {
    "--microphone-level-scale": levelPercent(rmsDbfs) / 100,
    "--microphone-peak": `${levelPercent(peakDbfs)}%`,
  } as CSSProperties;
  const formatDbfs = (value: number) => t("units.dbfs", { value: Math.round(value) });

  return (
    <div className={`microphone-level-setting ${speech ? "triggered" : ""}`}>
      <span className="microphone-level-status">{status}</span>
      <RangeField
        label={t("settings.audio.otherVoiceTriggerThreshold")}
        value={threshold}
        min={MIN_MICROPHONE_LEVEL_DBFS}
        max={MAX_MICROPHONE_THRESHOLD_DBFS}
        step={1}
        disabled={disabled}
        formatValue={formatDbfs}
        hideBounds
        trackSlot={(
          <span
            className="microphone-live-track"
            role="meter"
            aria-label={t("settings.audio.otherVoiceLevelMeterLabel")}
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
      <small>{t("settings.audio.otherVoiceTriggerThresholdDescription")}</small>
    </div>
  );
}
