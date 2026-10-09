import type { AudioDevice } from "../capture/types";
import type { Settings } from "./types";

type TranslateValidation = (key: string) => string;

const validationMessage: TranslateValidation = (key) => ({
  "validation.audio.outputUnavailable": "The selected system output device is no longer available",
  "validation.audio.microphoneUnavailable": "The selected microphone device is no longer available",
})[key] ?? key;

export function hasEnabledAudioSource(settings: Settings): boolean {
  return settings.audio.output.mode !== "disabled"
    || settings.audio.microphone.mode !== "disabled";
}

export function audioSelectionErrors(
  settings: Settings,
  devices: AudioDevice[],
  translate: TranslateValidation = validationMessage,
): string[] {
  const errors: string[] = [];
  const output = settings.audio.output;
  if (
    output.mode === "system"
    && output.device_id !== null
    && !devices.some((device) => device.is_loopback && device.id === output.device_id)
  ) {
    errors.push(translate("validation.audio.outputUnavailable"));
  }
  const microphone = settings.audio.microphone;
  if (
    microphone.mode === "device"
    && (
      microphone.device_id === null
      || !devices.some(
        (device) => !device.is_loopback && device.id === microphone.device_id,
      )
    )
  ) {
    errors.push(translate("validation.audio.microphoneUnavailable"));
  }
  return errors;
}
