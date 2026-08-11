import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic, RefreshCw, TriangleAlert, Volume2 } from "lucide-react";

import type { AudioDevice, AudioLevel, Settings } from "../../types";
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
  microphoneLevel,
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
  microphoneLevel: AudioLevel | null;
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
  const [microphoneTestBusy, setMicrophoneTestBusy] = useState(false);

  useEffect(() => () => {
    void onStopMicrophoneTest().catch(() => undefined);
  }, [onStopMicrophoneTest]);

  const toggleMicrophoneTest = async () => {
    if (microphoneTestBusy) return;
    setMicrophoneTestBusy(true);
    try {
      if (microphoneTestRunning) await onStopMicrophoneTest();
      else await onStartMicrophoneTest();
    } finally {
      setMicrophoneTestBusy(false);
    }
  };
  return (
        <div className="settings-section settings-section-active audio-section" id="settings-panel-audio" role="tabpanel" aria-labelledby="settings-tab-audio">
          <div className="section-heading">
            <div><h2>{t("settings.audio.title")}</h2><span>{devices.length ? t("settings.audio.devicesFound", { count: devices.length }) : t("settings.audio.waitingScan")}</span></div>
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
            note={t("settings.audio.otherVoiceDescription")}
            devices={outputDevices}
            devicesReady={devicesReady}
            selectedDeviceId={draft.audio.output.mode === "system" ? draft.audio.output.device_id : null}
            specialRows={[
              {
                key: "system",
                name: t("settings.audio.systemOutput"),
                description: t("settings.audio.systemOutputDescription"),
                chosen: draft.audio.output.mode === "system" && draft.audio.output.device_id === null,
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { mode: "system", device_id: null } },
                })),
              },
              {
                key: "vrchat",
                name: "VRChat",
                description: t("settings.audio.vrchatDescription"),
                chosen: draft.audio.output.mode === "vrchat",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: { ...current.audio, output: { mode: "vrchat", device_id: null } },
                })),
              },
              {
                key: "disabled",
                name: t("settings.audio.disableOtherVoices"),
                description: t("settings.audio.disableOtherVoicesDescription"),
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
            note={t("settings.audio.ownVoiceDescription")}
            beforeList={(
              <MicrophoneLevelSetting
                level={microphoneLevel}
                enabled={draft.audio.microphone.mode !== "disabled"}
                captureRunning={microphoneRunning}
                transcriptionRunning={transcriptionRunning}
                testing={microphoneTestRunning}
                busy={microphoneTestBusy}
                threshold={draft.audio.microphone.trigger_threshold_dbfs}
                disabled={saveState === "saving"}
                onToggleTest={() => void toggleMicrophoneTest()}
                onCommit={(triggerThresholdDbfs) => applySettings((current) => ({
                  ...current,
                  audio: {
                    ...current.audio,
                    microphone: {
                      ...current.audio.microphone,
                      trigger_threshold_dbfs: triggerThresholdDbfs,
                    },
                  },
                }))}
              />
            )}
            devices={microphoneDevices}
            devicesReady={devicesReady}
            selectedDeviceId={draft.audio.microphone.mode === "device" ? draft.audio.microphone.device_id : null}
            specialRows={[
              {
                key: "default",
                name: t("settings.audio.defaultMicrophone"),
                description: t("settings.audio.defaultMicrophoneDescription"),
                chosen: draft.audio.microphone.mode === "default",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: {
                    ...current.audio,
                    microphone: { ...current.audio.microphone, mode: "default", device_id: null },
                  },
                })),
              },
              {
                key: "disabled",
                name: t("settings.audio.disableMicrophone"),
                description: t("settings.audio.disableMicrophoneDescription"),
                chosen: draft.audio.microphone.mode === "disabled",
                onSelect: () => applySettings((current) => ({
                  ...current,
                  audio: {
                    ...current.audio,
                    microphone: { ...current.audio.microphone, mode: "disabled", device_id: null },
                  },
                })),
              },
            ]}
            disabled={saveState === "saving"}
            onSelectDevice={(id) => applySettings((current) => ({
              ...current,
              audio: {
                ...current.audio,
                microphone: { ...current.audio.microphone, mode: "device", device_id: id },
              },
            }))}
          />
        </div>
  );
}
