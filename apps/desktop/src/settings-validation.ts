import type {
  AsrCapabilities,
  AsrSettings,
  AudioDevice,
  Settings,
} from "./types";

type TranslateValidation = (key: string) => string;

const validationMessage: TranslateValidation = (key) => ({
  "validation.audio.outputUnavailable": "所选系统输出设备已失效，请重新选择",
  "validation.audio.microphoneUnavailable": "所选麦克风设备已失效，请重新选择",
  "validation.asr.cudaUnavailable": "CUDA 预检失败，请改用自动选择或 CPU",
  "validation.asr.invalidComputeType": "当前运行设备与计算类型组合无效",
})[key] ?? key;

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

export function audioSettingsChanged(
  previous: Settings,
  next: Settings,
): boolean {
  return previous.audio.sample_rate !== next.audio.sample_rate
    || previous.audio.output.mode !== next.audio.output.mode
    || previous.audio.output.device_id !== next.audio.output.device_id
    || previous.audio.microphone.mode !== next.audio.microphone.mode
    || previous.audio.microphone.device_id !== next.audio.microphone.device_id
    || JSON.stringify(previous.asr) !== JSON.stringify(next.asr);
}
