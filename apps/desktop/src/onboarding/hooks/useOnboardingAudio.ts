import { useState } from "react";
import { useTranslation } from "react-i18next";

import {
  audioSelectionErrors,
  hasEnabledAudioSource,
} from "../../settings/settings-validation";
import type { AudioDevice, Settings } from "../../types";

export function useOnboardingAudio({
  settings,
  devices,
  devicesReady,
  onSave,
  clearMessage,
  showError,
}: {
  settings: Settings;
  devices: AudioDevice[];
  devicesReady: boolean;
  onSave: (settings: Settings) => Promise<Settings>;
  clearMessage: () => void;
  showError: (reason: unknown, fallbackKey?: string) => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const outputDevices = devices.filter((device) => device.is_loopback);
  const microphoneDevices = devices.filter((device) => !device.is_loopback);
  const deviceErrors = devicesReady
    ? audioSelectionErrors(settings, devices, (key) => t(key))
    : [];
  const hasAudioSource = hasEnabledAudioSource(settings);
  const ready = devicesReady && deviceErrors.length === 0 && hasAudioSource;

  const updateSettings = async (update: (current: Settings) => Settings) => {
    setBusy(true);
    clearMessage();
    try {
      await onSave(update(settings));
    } catch (reason) {
      showError(reason, "errors.settings.apply");
    } finally {
      setBusy(false);
    }
  };

  return {
    busy,
    outputDevices,
    microphoneDevices,
    deviceErrors,
    hasAudioSource,
    ready,
    updateSettings,
  };
}
