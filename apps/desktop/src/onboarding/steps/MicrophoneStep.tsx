import { Mic, RefreshCw, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { DEFAULT_MICROPHONE_THRESHOLD_DBFS, type MicrophoneCalibrationResult } from "../../microphone-calibration";
import { MicrophoneLevelSetting } from "../../settings/sections/MicrophoneLevelSetting";
import type { MicrophoneCalibrationPhase } from "../../settings/hooks/useMicrophoneCalibration";
import type { AudioLevel } from "../../capture/types";
import type { Health } from "../../core-client/types";
import type { Settings } from "../../settings/types";

export function MicrophoneStep({
  settings,
  health,
  microphoneLevel,
  microphoneOperationBusy,
  audioBusy,
  microphoneReviewed,
  calibrationPhase,
  calibrationResult,
  onToggleMicrophoneTest,
  onCommitThreshold,
  onRestoreDefault,
  onApplySuggestedThreshold,
  onStartCalibration,
  onSkipCalibration,
}: {
  settings: Settings;
  health: Health | null;
  microphoneLevel: AudioLevel | null;
  microphoneOperationBusy: boolean;
  audioBusy: boolean;
  microphoneReviewed: boolean;
  calibrationPhase: MicrophoneCalibrationPhase;
  calibrationResult: MicrophoneCalibrationResult | null;
  onToggleMicrophoneTest: () => void;
  onCommitThreshold: (threshold: number) => void;
  onRestoreDefault: (threshold: number) => void;
  onApplySuggestedThreshold: () => void;
  onStartCalibration: () => void;
  onSkipCalibration: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="onboarding-step-content">
      {settings.audio.microphone.mode === "disabled" ? (
        <div className="onboarding-disabled-feature"><Mic size={28} /><h2>{t("onboarding.microphone.disabledTitle")}</h2><p>{t("onboarding.microphone.disabledDescription")}</p></div>
      ) : (
        <>
          <div className="onboarding-intro"><p>{t("onboarding.microphone.description")}</p></div>
          <MicrophoneLevelSetting
            level={microphoneLevel}
            enabled
            captureRunning={false}
            transcriptionRunning={health?.capture_requested ?? false}
            testing={health?.microphone_test_running ?? false}
            busy={microphoneOperationBusy}
            threshold={settings.audio.microphone.trigger_threshold_dbfs}
            disabled={audioBusy}
            onToggleTest={onToggleMicrophoneTest}
            onCommit={onCommitThreshold}
          />
          <div className="onboarding-calibration-card">
            <div className="onboarding-panel-heading"><Sparkles size={18} /><div><strong>{t("onboarding.microphone.autoTitle")}</strong><small>{t("onboarding.microphone.autoDescription")}</small></div></div>
            <div className={`onboarding-calibration-status phase-${calibrationPhase}`} role="status" aria-live="polite">
              <span><i /></span>
              <div>
                <strong>{t(`onboarding.microphone.phase.${calibrationPhase}`)}</strong>
                <small>{calibrationPhase === "ready" && calibrationResult
                  ? t("onboarding.microphone.result", { noise: calibrationResult.noiseLevel, speech: calibrationResult.speechLevel, threshold: calibrationResult.threshold })
                  : t(`onboarding.microphone.phaseDescription.${calibrationPhase}`)}</small>
              </div>
            </div>
            <div className="onboarding-calibration-actions">
              <button className="secondary-button" type="button" disabled={microphoneOperationBusy || audioBusy} onClick={() => onRestoreDefault(DEFAULT_MICROPHONE_THRESHOLD_DBFS)}>{t("onboarding.microphone.restoreDefault")}</button>
              {calibrationResult && <button className="secondary-button" type="button" disabled={audioBusy} onClick={onApplySuggestedThreshold}>{t("onboarding.microphone.applySuggestion")}</button>}
              <button className="primary-button" type="button" disabled={microphoneOperationBusy || audioBusy} onClick={onStartCalibration}>{microphoneOperationBusy ? <RefreshCw className="spin" size={15} /> : <Sparkles size={15} />}{calibrationPhase === "idle" ? t("onboarding.microphone.startCalibration") : t("onboarding.microphone.retryCalibration")}</button>
            </div>
          </div>
          {!microphoneReviewed && <button className="onboarding-text-action" type="button" onClick={onSkipCalibration}>{t("onboarding.microphone.skipCalibration")}</button>}
        </>
      )}
    </div>
  );
}
