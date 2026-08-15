import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic, RefreshCw, Sparkles, TriangleAlert, Volume2 } from "lucide-react";

import { useMicrophoneCalibration } from "../../hooks/useMicrophoneCalibration";
import { DEFAULT_MICROPHONE_THRESHOLD_DBFS } from "../../microphone-calibration";
import { useAudioLevel } from "../../realtime-state";
import type { AudioDevice, Settings } from "../../types";
import type { ApplySettings, SaveState } from "../settings-types";
import { DeviceGroup } from "../SettingsControls";
import { MicrophoneLevelSetting } from "./MicrophoneLevelSetting";

export function AudioSettingsSection({
  draft,
  devices,
  devicesReady,
  deviceErrors,
  outputDevices,
  microphoneDevices,
  microphoneRunning,
  transcriptionRunning,
  microphoneTestRunning,
  saveState,
  onRefresh,
  applySettings,
  onStartMicrophoneTest,
  onStopMicrophoneTest,
}: {
  draft: Settings;
  devices: AudioDevice[];
  devicesReady: boolean;
  deviceErrors: string[];
  outputDevices: AudioDevice[];
  microphoneDevices: AudioDevice[];
  microphoneRunning: boolean;
  transcriptionRunning: boolean;
  microphoneTestRunning: boolean;
  saveState: SaveState;
  onRefresh: () => Promise<void>;
  applySettings: ApplySettings;
  onStartMicrophoneTest: () => Promise<void>;
  onStopMicrophoneTest: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const microphoneLevel = useAudioLevel("microphone");
  const [microphoneTestBusy, setMicrophoneTestBusy] = useState(false);
  const calibration = useMicrophoneCalibration({
    level: microphoneLevel,
    testing: microphoneTestRunning,
    onStartTest: onStartMicrophoneTest,
  });
  const microphoneOperationBusy = microphoneTestBusy || calibration.calibrating;

  useEffect(() => () => {
    void onStopMicrophoneTest().catch(() => undefined);
  }, [onStopMicrophoneTest]);

  const toggleMicrophoneTest = async () => {
    if (microphoneOperationBusy) return;
    calibration.reset();
    setMicrophoneTestBusy(true);
    try {
      if (microphoneTestRunning) await onStopMicrophoneTest();
      else await onStartMicrophoneTest();
    } finally {
      setMicrophoneTestBusy(false);
    }
  };

  const startCalibration = async () => {
    if (microphoneOperationBusy) return;
    try {
      await calibration.start();
    } catch {
      // The shared core session reports the microphone-test error.
    }
  };

  const updateMicrophoneThreshold = (threshold: number) => applySettings((current) => ({
    ...current,
    audio: {
      ...current.audio,
      microphone: {
        ...current.audio.microphone,
        trigger_threshold_dbfs: threshold,
      },
    },
  }));

  const calibrationDisabled = (
    draft.audio.microphone.mode === "disabled"
    || transcriptionRunning
    || saveState === "saving"
  );
  return (
        <div className="settings-section settings-section-active audio-section" id="settings-panel-audio" role="tabpanel" aria-labelledby="settings-tab-audio">
          <div className="section-heading">
            <div><Volume2 size={18} /><h2>{t("settings.audio.title")}</h2><span>{devices.length ? t("settings.audio.devicesFound", { count: devices.length }) : t("settings.audio.waitingScan")}</span></div>
            <button className="secondary-button" type="button" onClick={() => void onRefresh()}><RefreshCw size={15} />{t("settings.audio.rescan")}</button>
          </div>

          {deviceErrors.map((message) => (
            <p className="settings-validation-error" role="alert" key={message}>
              <TriangleAlert size={15} />{message}
            </p>
          ))}
          <DeviceGroup
            icon={<Volume2 size={18} />}
            title={t("settings.audio.otherVoice")}
            devices={outputDevices}
            devicesReady={devicesReady}
            selectedDeviceId={draft.audio.output.mode === "system" ? draft.audio.output.device_id : null}
            specialRows={[
              {
                key: "system",
                name: t("settings.audio.systemOutput"),
                chosen: draft.audio.output.mode === "system" && draft.audio.output.device_id === null,
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { mode: "system", device_id: null } },
                })),
              },
              {
                key: "vrchat",
                name: "VRChat",
                chosen: draft.audio.output.mode === "vrchat",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { mode: "vrchat", device_id: null } },
                })),
              },
              {
                key: "disabled",
                name: t("settings.audio.disableOtherVoices"),
                chosen: draft.audio.output.mode === "disabled",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { mode: "disabled", device_id: null } },
                })),
              },
            ]}
            disabled={saveState === "saving"}
            onSelectDevice={(id) => applySettings((current) => ({
              ...current,
              audio: { ...current.audio, output: { mode: "system", device_id: id } },
            }))}
          />
          <DeviceGroup
            icon={<Mic size={18} />}
            title={t("settings.audio.ownVoice")}
            beforeList={(
              <>
                <MicrophoneLevelSetting
                  level={microphoneLevel}
                  enabled={draft.audio.microphone.mode !== "disabled"}
                  captureRunning={microphoneRunning}
                  transcriptionRunning={transcriptionRunning}
                  testing={microphoneTestRunning}
                  busy={microphoneOperationBusy}
                  threshold={draft.audio.microphone.trigger_threshold_dbfs}
                  disabled={saveState === "saving"}
                  onToggleTest={() => void toggleMicrophoneTest()}
                  onCommit={(threshold) => {
                    calibration.reset();
                    updateMicrophoneThreshold(threshold);
                  }}
                />
                <div className="microphone-calibration-card">
                  <div className="microphone-calibration-heading">
                    <Sparkles size={18} />
                    <div>
                      <strong>{t("onboarding.microphone.autoTitle")}</strong>
                    </div>
                  </div>
                  <div className={`microphone-calibration-status phase-${calibration.phase}`} role="status" aria-live="polite">
                    <span><i /></span>
                    <div>
                      <strong>{t(`onboarding.microphone.phase.${calibration.phase}`)}</strong>
                      <small>{calibration.phase === "ready" && calibration.result
                        ? t("onboarding.microphone.result", {
                            noise: calibration.result.noiseLevel,
                            speech: calibration.result.speechLevel,
                            threshold: calibration.result.threshold,
                          })
                        : t(`onboarding.microphone.phaseDescription.${calibration.phase}`)}</small>
                    </div>
                  </div>
                  <div className="microphone-calibration-actions">
                    <button
                      className="secondary-button"
                      type="button"
                      disabled={microphoneOperationBusy || saveState === "saving"}
                      onClick={() => {
                        calibration.reset();
                        updateMicrophoneThreshold(DEFAULT_MICROPHONE_THRESHOLD_DBFS);
                      }}
                    >
                      {t("onboarding.microphone.restoreDefault")}
                    </button>
                    {calibration.result && (
                      <button
                        className="secondary-button"
                        type="button"
                        disabled={saveState === "saving"}
                        onClick={() => updateMicrophoneThreshold(calibration.result!.threshold)}
                      >
                        {t("onboarding.microphone.applySuggestion")}
                      </button>
                    )}
                    <button
                      className="primary-button"
                      type="button"
                      disabled={microphoneOperationBusy || calibrationDisabled}
                      onClick={() => void startCalibration()}
                    >
                      {calibration.phase === "idle"
                        ? t("onboarding.microphone.startCalibration")
                        : t("onboarding.microphone.retryCalibration")}
                    </button>
                  </div>
                </div>
              </>
            )}
            devices={microphoneDevices}
            devicesReady={devicesReady}
            selectedDeviceId={draft.audio.microphone.mode === "device" ? draft.audio.microphone.device_id : null}
            specialRows={[
              {
                key: "default",
                name: t("settings.audio.defaultMicrophone"),
                chosen: draft.audio.microphone.mode === "default",
                onSelect: () => {
                  calibration.reset();
                  applySettings((current) => ({
                    ...current,
                    audio: {
                      ...current.audio,
                      microphone: { ...current.audio.microphone, mode: "default", device_id: null },
                    },
                  }));
                },
              },
              {
                key: "disabled",
                name: t("settings.audio.disableMicrophone"),
                chosen: draft.audio.microphone.mode === "disabled",
                onSelect: () => {
                  calibration.reset();
                  applySettings((current) => ({
                    ...current,
                    audio: {
                      ...current.audio,
                      microphone: { ...current.audio.microphone, mode: "disabled", device_id: null },
                    },
                  }));
                },
              },
            ]}
            disabled={saveState === "saving" || microphoneOperationBusy}
            onSelectDevice={(id) => {
              calibration.reset();
              applySettings((current) => ({
                ...current,
                audio: {
                  ...current.audio,
                  microphone: { ...current.audio.microphone, mode: "device", device_id: id },
                },
              }));
            }}
          />
        </div>
  );
}
