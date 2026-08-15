import type {
  AsrCapabilities,
  AsrSettings,
  AudioDevice,
  Settings,
} from "./types";

type TranslateValidation = (key: string) => string;

const validationMessage: TranslateValidation = (key) => ({
  "validation.audio.outputUnavailable": "The selected system output device is no longer available",
  "validation.audio.microphoneUnavailable": "The selected microphone device is no longer available",
  "validation.asr.cudaUnavailable": "CUDA preflight failed; use automatic selection or CPU",
  "validation.asr.invalidComputeType": "The selected runtime device and compute type are incompatible",
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

export function validComputeTypes(
  capabilities: AsrCapabilities | null,
  device: AsrSettings["local"]["device"],
): AsrSettings["local"]["compute_type"][] {
  return capabilities?.compute_types[device] ?? ["int8"];
}

export function asrSelectionError(
  settings: Settings,
  capabilities: AsrCapabilities | null,
  translate: TranslateValidation = validationMessage,
): string | null {
  if (!capabilities) return null;
  if (settings.asr.local.device === "cuda" && !capabilities.cuda.available) {
    return translate("validation.asr.cudaUnavailable");
  }
  if (!validComputeTypes(capabilities, settings.asr.local.device).includes(settings.asr.local.compute_type)) {
    return translate("validation.asr.invalidComputeType");
  }
  return null;
}
