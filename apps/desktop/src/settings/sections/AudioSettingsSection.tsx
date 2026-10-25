import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic, RefreshCw, Sparkles, TriangleAlert, Volume2 } from "lucide-react";

import { useMicrophoneCalibration } from "../hooks/useMicrophoneCalibration";
import { DEFAULT_MICROPHONE_THRESHOLD_DBFS } from "../../capture/microphone-calibration";
import { useAudioLevel } from "../../core-client/realtime-state";
import type { AudioDevice } from "../../capture/types";
import type { Settings } from "../types";
import type { ApplySettings, SaveState } from "../settings-types";
import { DeviceGroup } from "../SettingsControls";
import { MicrophoneLevelSetting } from "./MicrophoneLevelSetting";
import { OtherVoiceLevelSetting } from "./OtherVoiceLevelSetting";
import { DEFAULT_PLAYER_AUDIO, PlayerAudioSettings } from "./PlayerAudioSettings";
import type { PlayerAudioStatus } from "../../core-client/types";
import type { PlayerAudioStart } from "../../capture/usePlayerAudioStart";
import { MemoryCompressionControls } from "../../capture/MemoryCompressionControls";

export function AudioSettingsSection({
  draft,
  playerAudioStatus,
  playerAudioStart,
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
  playerAudioStatus?: PlayerAudioStatus | null;
  playerAudioStart?: PlayerAudioStart;
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
  const speakerLevel = useAudioLevel("speaker");
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
    } catch {
      // The shared core session reports the microphone-test error.
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

  const updateOutputThreshold = (threshold: number) => applySettings((current) => ({
    ...current,
    audio: {
      ...current.audio,
      output: {
        ...current.audio.output,
        trigger_threshold_dbfs: threshold,
      },
    },
  }));

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
            beforeList={(
              <OtherVoiceLevelSetting
                level={speakerLevel}
                enabled={draft.audio.output.mode !== "disabled"}
                captureRunning={transcriptionRunning}
                threshold={draft.audio.output.trigger_threshold_dbfs}
                disabled={draft.audio.output.mode === "disabled"}
                onCommit={updateOutputThreshold}
              />
            )}
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
                  audio: { ...current.audio, output: { ...current.audio.output, mode: "system", device_id: null } },
                })),
              },
              {
                key: "vrchat",
                name: "VRChat",
                chosen: draft.audio.output.mode === "vrchat",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { ...current.audio.output, mode: "vrchat", device_id: null } },
                })),
              },
              {
                key: "vrchat_players",
                name: t("settings.audio.players.title"),
                description: t("settings.audio.players.nameHint"),
                chosen: draft.audio.output.mode === "vrchat_players",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { ...current.audio.output, mode: "vrchat_players", device_id: null } },
                })),
              },
              {
                key: "disabled",
                name: t("settings.audio.disableOtherVoices"),
                chosen: draft.audio.output.mode === "disabled",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { ...current.audio.output, mode: "disabled", device_id: null } },
                })),
              },
            ]}
            disabled={false}
            onSelectDevice={(id) => applySettings((current) => ({
              ...current,
              audio: { ...current.audio, output: { ...current.audio.output, mode: "system", device_id: id } },
            }))}
          />
          {draft.audio.output.mode === "vrchat_players" && <PlayerAudioSettings
            value={draft.audio.output.player_audio} status={playerAudioStatus} disabled={saveState === "saving"}
            onChange={(patch) => applySettings((current) => ({
              ...current, audio: { ...current.audio, output: { ...current.audio.output,
                player_audio: { ...(current.audio.output.player_audio ?? DEFAULT_PLAYER_AUDIO), ...patch },
              } },
            }))} />}
          {playerAudioStart && playerAudioStart.memory.status?.supported !== false
            && <MemoryCompressionControls memory={playerAudioStart.memory} />}
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
                  disabled={false}
                  testDisabled={saveState === "saving"}
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
                      disabled={microphoneOperationBusy}
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
            disabled={microphoneOperationBusy}
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
