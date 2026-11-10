import { Mic, RefreshCw, Volume2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { DeviceGroup } from "../../settings/SettingsControls";
import type { AudioDevice } from "../../capture/types";
import type { Settings } from "../../settings/types";
import { CapabilityNotice, usePlatformCapabilities } from "../../core-client/capabilities";

export function AudioStep({
  settings,
  outputDevices,
  microphoneDevices,
  devicesReady,
  deviceErrors,
  hasAudioSource,
  audioBusy,
  onRefreshDevices,
  onUpdateSettings,
  onSetMicrophoneReviewed,
}: {
  settings: Settings;
  outputDevices: AudioDevice[];
  microphoneDevices: AudioDevice[];
  devicesReady: boolean;
  deviceErrors: string[];
  hasAudioSource: boolean;
  audioBusy: boolean;
  onRefreshDevices: () => void;
  onUpdateSettings: (update: (current: Settings) => Settings) => void;
  onSetMicrophoneReviewed: (reviewed: boolean) => void;
}) {
  const { t } = useTranslation();
  const { core } = usePlatformCapabilities();

  return (
    <div className="onboarding-step-content">
      <div className="onboarding-section-heading">
        <div><p>{t("onboarding.audio.description")}</p></div>
        <button className="secondary-button" type="button" disabled={audioBusy} onClick={onRefreshDevices}><RefreshCw size={15} />{t("settings.audio.rescan")}</button>
      </div>
      {deviceErrors.map((error) => <p className="settings-validation-error" role="alert" key={error}>{error}</p>)}
      {devicesReady && !hasAudioSource && (
        <p className="settings-validation-error" role="alert">{t("onboarding.audio.sourceRequired")}</p>
      )}
      <CapabilityNotice capability={core?.audio.system_output} />
      <DeviceGroup
        icon={<Volume2 size={18} />}
        title={t("settings.audio.otherVoice")}
        note={t("settings.audio.otherVoiceDescription")}
        devices={outputDevices}
        devicesReady={devicesReady}
        selectedDeviceId={!settings.audio.output.device_key && settings.audio.output.mode === "system" ? settings.audio.output.device_id : null}
        selectedDeviceKey={settings.audio.output.mode === "system" ? settings.audio.output.device_key : null}
        specialRows={[
          { key: "system", name: t("settings.audio.systemOutput"), description: t("settings.audio.systemOutputDescription"), disabled: !core?.audio.system_output.supported, chosen: settings.audio.output.mode === "system" && settings.audio.output.device_id === null && !settings.audio.output.device_key, onSelect: () => onUpdateSettings((current) => ({ ...current, audio: { ...current.audio, output: { ...current.audio.output, mode: "system", device_id: null, device_key: null } } })) },
          { key: "vrchat", name: "VRChat", description: core?.audio.application_output.supported ? t("settings.audio.vrchatDescription") : t("platform.application_audio_unavailable"), disabled: !core?.audio.application_output.supported, chosen: settings.audio.output.mode === "vrchat", onSelect: () => onUpdateSettings((current) => ({ ...current, audio: { ...current.audio, output: { ...current.audio.output, mode: "vrchat", device_id: null, device_key: null } } })) },
          { key: "disabled", name: t("settings.audio.disableOtherVoices"), description: t("settings.audio.disableOtherVoicesDescription"), chosen: settings.audio.output.mode === "disabled", onSelect: () => onUpdateSettings((current) => ({ ...current, audio: { ...current.audio, output: { ...current.audio.output, mode: "disabled", device_id: null, device_key: null } } })) },
        ]}
        disabled={audioBusy}
        onSelectDevice={(device) => onUpdateSettings((current) => ({ ...current, audio: { ...current.audio, output: { ...current.audio.output, mode: "system", device_id: device.device_key ? null : device.id, device_key: device.device_key ?? null } } }))}
      />
      {(core?.audio.microphone.reason_code !== core?.audio.system_output.reason_code || core?.audio.system_output.available) && <CapabilityNotice capability={core?.audio.microphone} />}
      <DeviceGroup
        icon={<Mic size={18} />}
        title={t("settings.audio.ownVoice")}
        note={t("settings.audio.ownVoiceDescription")}
        devices={microphoneDevices}
        devicesReady={devicesReady}
        selectedDeviceId={!settings.audio.microphone.device_key && settings.audio.microphone.mode !== "disabled" ? settings.audio.microphone.device_id : null}
        selectedDeviceKey={settings.audio.microphone.mode !== "disabled" ? settings.audio.microphone.device_key : null}
        specialRows={[
          { key: "default", name: t("settings.audio.defaultMicrophone"), description: t("settings.audio.defaultMicrophoneDescription"), disabled: !core?.audio.microphone.supported, chosen: settings.audio.microphone.mode === "default" && settings.audio.microphone.device_id === null && !settings.audio.microphone.device_key, onSelect: () => { onSetMicrophoneReviewed(false); onUpdateSettings((current) => ({ ...current, audio: { ...current.audio, microphone: { ...current.audio.microphone, mode: "default", device_id: null, device_key: null } } })); } },
          { key: "disabled", name: t("settings.audio.disableMicrophone"), description: t("settings.audio.disableMicrophoneDescription"), chosen: settings.audio.microphone.mode === "disabled", onSelect: () => { onSetMicrophoneReviewed(true); onUpdateSettings((current) => ({ ...current, audio: { ...current.audio, microphone: { ...current.audio.microphone, mode: "disabled", device_id: null, device_key: null } } })); } },
        ]}
        disabled={audioBusy}
        onSelectDevice={(device) => { onSetMicrophoneReviewed(false); onUpdateSettings((current) => ({ ...current, audio: { ...current.audio, microphone: { ...current.audio.microphone, mode: "device", device_id: device.device_key ? null : device.id, device_key: device.device_key ?? null } } })); }}
      />
    </div>
  );
}
