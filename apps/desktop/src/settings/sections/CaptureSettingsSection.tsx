import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic, RefreshCw, Sparkles, TriangleAlert, Volume2 } from "lucide-react";

import { useMicrophoneCalibration } from "../hooks/useMicrophoneCalibration";
import { DEFAULT_MICROPHONE_THRESHOLD_DBFS } from "../../capture/microphone-calibration";
import { useAudioLevel } from "../../core-client/realtime-state";
import type { AudioDevice } from "../../capture/types";
import type { Settings, TranslationSettings } from "../types";
import type { ApplySettings, SaveState } from "../settings-types";
import { DeviceGroup } from "../SettingsControls";
import { MicrophoneLevelSetting } from "./MicrophoneLevelSetting";
import { OtherVoiceLevelSetting } from "./OtherVoiceLevelSetting";
import { VrchatChatboxSettings } from "./VrchatChatboxSettings";
import { DEFAULT_PLAYER_AUDIO, PlayerAudioSettings } from "./PlayerAudioSettings";
import type { ChatboxStatus, PlayerAudioStatus } from "../../core-client/types";
import type { ApiProfileView } from "../../providers/types";
import type { PlayerAudioStart } from "../../capture/usePlayerAudioStart";
import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { CapabilityNotice, usePlatformCapabilities } from "../../core-client/capabilities";

export function CaptureSettingsSection({
  draft,
  playerAudioStatus,
  playerAudioStart,
  chatboxStatus,
  translation = draft.translation,
  profiles = [],
  onOpenTranslation = () => undefined,
  devices,
  devicesReady,
  deviceErrors,
  outputDevices,
  microphoneDevices,
  microphoneRunning,
  transcriptionRunning,
  microphoneTestRunning,
  saveState,
  saveMessage,
  onRefresh,
  applySettings,
  onStartMicrophoneTest,
  onStopMicrophoneTest,
}: {
  draft: Settings;
  playerAudioStatus?: PlayerAudioStatus | null;
  playerAudioStart?: PlayerAudioStart;
  chatboxStatus?: ChatboxStatus | null;
  translation?: TranslationSettings;
  profiles?: ApiProfileView[];
  onOpenTranslation?: () => void;
  devices: AudioDevice[];
  devicesReady: boolean;
  deviceErrors: string[];
  outputDevices: AudioDevice[];
  microphoneDevices: AudioDevice[];
  microphoneRunning: boolean;
  transcriptionRunning: boolean;
  microphoneTestRunning: boolean;
  saveState: SaveState;
  saveMessage?: string;
  onRefresh: () => Promise<void>;
  applySettings: ApplySettings;
  onStartMicrophoneTest: () => Promise<void>;
  onStopMicrophoneTest: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const { core } = usePlatformCapabilities();
  const microphoneLevel = useAudioLevel("microphone");
  const speakerLevel = useAudioLevel("speaker");
  const [microphoneTestBusy, setMicrophoneTestBusy] = useState(false);
  const [networkRequiredOpen, setNetworkRequiredOpen] = useState(false);
  const networkEnabled = draft.audio.output.player_audio?.enabled === true;
  const networkSupported = core?.chatbox.network_receive.available === true;
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
    if (microphoneOperationBusy || (!microphoneTestRunning && !core?.audio.microphone.available)) return;
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
    if (microphoneOperationBusy || !core?.audio.microphone.available) return;
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
    !core?.audio.microphone.available ||
    draft.audio.microphone.mode === "disabled"
    || transcriptionRunning
    || saveState === "saving"
  );
  return (
        <div className="settings-section settings-section-active audio-section" id="settings-panel-audio" role="tabpanel" aria-labelledby="settings-tab-audio">
          <div className="section-heading">
            <div><Volume2 size={18} /><h2>{t("settings.audio.title")}</h2><span>{core && !core.audio.system_output.supported ? t("common.unavailable") : devices.length ? t("settings.audio.devicesFound", { count: devices.length }) : t("settings.audio.waitingScan")}</span></div>
            <button className="secondary-button" type="button" onClick={() => void onRefresh()}><RefreshCw size={15} />{t("settings.audio.rescan")}</button>
          </div>

          {deviceErrors.map((message) => (
            <p className="settings-validation-error" role="alert" key={message}>
              <TriangleAlert size={15} />{message}
            </p>
          ))}
          <CapabilityNotice capability={core?.audio.system_output} />
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
            selectedDeviceId={!draft.audio.output.device_key && draft.audio.output.mode === "system" ? draft.audio.output.device_id : null}
            selectedDeviceKey={draft.audio.output.mode === "system" ? draft.audio.output.device_key : null}
            specialRows={[
              {
                key: "system",
                name: t("settings.audio.systemOutput"),
                disabled: !core?.audio.system_output.supported,
                chosen: draft.audio.output.mode === "system" && draft.audio.output.device_id === null && !draft.audio.output.device_key,
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { ...current.audio.output, mode: "system", device_id: null, device_key: null } },
                })),
              },
              {
                key: "vrchat",
                name: "VRChat",
                disabled: !core?.audio.application_output.supported,
                description: !core?.audio.application_output.supported ? t("platform.application_audio_unavailable") : undefined,
                chosen: draft.audio.output.mode === "vrchat",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { ...current.audio.output, mode: "vrchat", device_id: null, device_key: null } },
                })),
              },
              {
                key: "vrchat_players",
                name: t("settings.audio.players.title"),
                description: t("settings.audio.players.nameHint"),
                disabled: !core?.audio.player_audio.available,
                chosen: draft.audio.output.mode === "vrchat_players",
                onSelect: () => {
                  if (!networkEnabled || !networkSupported) {
                    setNetworkRequiredOpen(true);
                    return;
                  }
                  applySettings((current) => ({
                    ...current,
                    audio: { ...current.audio, output: { ...current.audio.output, mode: "vrchat_players", device_id: null, device_key: null } },
                  }));
                },
              },
              {
                key: "disabled",
                name: t("settings.audio.disableOtherVoices"),
                chosen: draft.audio.output.mode === "disabled",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { ...current.audio.output, mode: "disabled", device_id: null, device_key: null } },
                })),
              },
            ]}
            disabled={false}
            onSelectDevice={(device) => applySettings((current) => ({
              ...current,
              audio: { ...current.audio, output: { ...current.audio.output, mode: "system", device_id: device.device_key ? null : device.id, device_key: device.device_key ?? null } },
            }))}
          />
          {(core?.audio.microphone.reason_code !== core?.audio.system_output.reason_code || core?.audio.system_output.available) && <CapabilityNotice capability={core?.audio.microphone} />}
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
                  disabled={!core?.audio.microphone.available}
                  testDisabled={saveState === "saving" || !core?.audio.microphone.available}
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
            selectedDeviceId={!draft.audio.microphone.device_key && draft.audio.microphone.mode !== "disabled" ? draft.audio.microphone.device_id : null}
            selectedDeviceKey={draft.audio.microphone.mode !== "disabled" ? draft.audio.microphone.device_key : null}
            specialRows={[
              {
                key: "default",
                name: t("settings.audio.defaultMicrophone"),
                disabled: !core?.audio.microphone.supported,
                chosen: draft.audio.microphone.mode === "default" && draft.audio.microphone.device_id === null && !draft.audio.microphone.device_key,
                onSelect: () => {
                  calibration.reset();
                  applySettings((current) => ({
                    ...current,
                    audio: {
                      ...current.audio,
                      microphone: { ...current.audio.microphone, mode: "default", device_id: null, device_key: null },
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
                      microphone: { ...current.audio.microphone, mode: "disabled", device_id: null, device_key: null },
                    },
                  }));
                },
              },
            ]}
            disabled={microphoneOperationBusy}
            onSelectDevice={(device) => {
              calibration.reset();
              applySettings((current) => ({
                ...current,
                audio: {
                  ...current.audio,
                  microphone: { ...current.audio.microphone, mode: "device", device_id: device.device_key ? null : device.id, device_key: device.device_key ?? null },
                },
              }));
            }}
          />
          <CapabilityNotice capability={core?.chatbox.network_receive} />
          <fieldset className="platform-capability-section">
          <VrchatChatboxSettings draft={draft} translation={translation} profiles={profiles} status={chatboxStatus}
            supported={networkSupported} saveState={saveState} saveMessage={saveMessage}
            applySettings={applySettings} onOpenTranslation={onOpenTranslation} />
          <PlayerAudioSettings value={draft.audio.output.player_audio} status={playerAudioStatus}
            memory={playerAudioStart?.memory}
            disabled={!networkSupported}
            onChange={(patch) => applySettings((current) => ({
              ...current,
              audio: { ...current.audio, output: { ...current.audio.output,
                player_audio: { ...(current.audio.output.player_audio ?? DEFAULT_PLAYER_AUDIO), ...patch },
              } },
            }))} />
          </fieldset>
          {networkRequiredOpen && <ConfirmDialog
            title={t("settings.capture.networkTitle")}
            message={t(networkSupported ? "settings.capture.networkRequired" : "settings.capture.unsupported")}
            onClose={() => setNetworkRequiredOpen(false)} />}
        </div>
  );
}
