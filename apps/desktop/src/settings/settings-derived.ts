import type { TFunction } from "i18next";

import type { AnkiStatus } from "../anki/types";
import type {
  ApiProfileView,
  AsrCapabilities,
  AsrModelRecord,
  ProviderDefinition,
} from "../providers/types";
import type { Settings } from "./types";
import {
  LOCAL_RECOGNITION_SOURCE,
  recognitionSourceValue as dynamicRecognitionSourceValue,
  selectRecognitionProfile,
} from "../recognition-services.ts";
import type {
  DebugRow,
  SettingOption,
} from "./settings-types";

export const MODEL_PRESENTATION: Record<AsrModelRecord["id"], {
  name: string;
  descriptionKey: string;
}> = {
  tiny: { name: "Tiny", descriptionKey: "settings.recognition.models.tiny" },
  base: { name: "Base", descriptionKey: "settings.recognition.models.base" },
  small: { name: "Small", descriptionKey: "settings.recognition.models.small" },
  medium: { name: "Medium", descriptionKey: "settings.recognition.models.medium" },
  "large-v3": { name: "Large v3", descriptionKey: "settings.recognition.models.largeV3" },
};

export function showsLocalRecognitionSettings(
  backend: Settings["asr"]["backend"],
): boolean {
  return backend === "local_whisper";
}

export { LOCAL_RECOGNITION_SOURCE };

export function recognitionSourceValue(asr: Settings["asr"]): string {
  return dynamicRecognitionSourceValue(asr);
}

export function selectRecognitionSource(
  asr: Settings["asr"],
  source: string,
  profiles: ApiProfileView[],
  definitions: ProviderDefinition[],
): Settings["asr"] {
  return selectRecognitionProfile(asr, source, profiles, definitions);
}

export function formatBytes(bytes: number, locale: string): string {
  if (bytes < 1_000_000) {
    return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(Math.max(0, bytes / 1_000))} KB`;
  }
  if (bytes < 1_000_000_000) {
    return `${new Intl.NumberFormat(locale, {
      maximumFractionDigits: bytes < 100_000_000 ? 1 : 0,
    }).format(bytes / 1_000_000)} MB`;
  }
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(bytes / 1_000_000_000)} GB`;
}

export function classifyModels(
  managedModels: AsrModelRecord[],
  capabilities: AsrCapabilities | null,
  currentModel: Settings["asr"]["local"]["model"],
  modelsReady: boolean,
) {
  const installed = managedModels.filter((model) =>
    ["downloaded", "loading", "ready"].includes(model.status),
  );
  const downloading = managedModels.filter((model) => model.status === "downloading");
  const selectable = modelsReady
    ? managedModels.filter((model) =>
        model.id === currentModel
        || ["downloaded", "loading", "ready"].includes(model.status),
      )
    : (capabilities?.models ?? []).filter((model) =>
        model.id === currentModel || model.status !== "not_downloaded",
      );
  return { installed, downloading, selectable };
}

export function createAnkiOptions(
  status: AnkiStatus | null,
  settings: Settings["anki"],
): {
  decks: string[];
  models: SettingOption[];
  frontFields: SettingOption[];
  backFields: SettingOption[];
} {
  const options = (values: string[], current: string) =>
    Array.from(new Set([current, ...values])).map((value) => ({ value, label: value }));

  return {
    decks: Array.from(new Set([settings.deck, ...(status?.decks ?? [])])),
    models: options(status?.models ?? [], settings.model),
    frontFields: options(
      (status?.fields ?? []).filter((field) => field !== settings.back_field),
      settings.front_field,
    ),
    backFields: options(
      (status?.fields ?? []).filter((field) => field !== settings.front_field),
      settings.back_field,
    ),
  };
}

export function modelStatusLabel(
  status: string | undefined,
  t: TFunction,
): string {
  if (status === "not_downloaded") return t("settings.recognition.modelStatus.notDownloaded");
  if (status === "loading") return t("settings.recognition.modelStatus.loading");
  if (status === "error") return t("settings.recognition.modelStatus.error");
  if (status) return t("settings.recognition.modelStatus.ready");
  return t("settings.recognition.modelStatus.checking");
}

export function createDebugRows({
  draft,
  modelStatus,
  asrCapabilities,
  disabled,
  outputDeviceCount,
  microphoneDeviceCount,
  dictionaryCount,
  locale,
  t,
}: {
  draft: Settings;
  modelStatus: string;
  asrCapabilities: AsrCapabilities | null;
  disabled: boolean;
  outputDeviceCount: number;
  microphoneDeviceCount: number;
  dictionaryCount: number;
  locale: string;
  t: TFunction;
}): DebugRow[] {
  return [
    { label: t("settings.debug.schema"), value: `v${draft.schema_version}` },
    { label: t("settings.debug.coreAddress"), value: `${draft.server.host}:${draft.server.port}` },
    { label: t("settings.debug.databasePath"), value: draft.storage.database_path },
    { label: t("settings.debug.modelDirectory"), value: draft.storage.model_directory },
    { label: t("settings.debug.sampleRate"), value: `${new Intl.NumberFormat(locale).format(draft.audio.sample_rate)} Hz` },
    { label: t("settings.debug.silence"), value: t("units.seconds", { value: draft.vad.silence_seconds.toFixed(1) }) },
    { label: t("settings.debug.maxSegment"), value: t("units.seconds", { value: draft.vad.max_speech_seconds }) },
    {
      label: t("settings.debug.historyStorageLimit"),
      value: formatBytes(draft.storage.subtitle_history_max_bytes, locale),
    },
    { label: t("settings.debug.modelStatus"), value: modelStatus },
    {
      label: t("settings.debug.cuda"),
      value: asrCapabilities?.cuda.available
        ? t("settings.debug.availableDevices", { count: asrCapabilities.cuda.device_count })
        : t("common.unavailable"),
    },
    {
      label: t("settings.debug.transcription"),
      value: disabled ? t("status.transcribing") : t("status.stopped"),
    },
    {
      label: t("settings.debug.audioDevices"),
      value: t("settings.debug.audioDeviceCounts", {
        outputs: outputDeviceCount,
        microphones: microphoneDeviceCount,
      }),
    },
    {
      label: t("settings.debug.dictionaries"),
      value: dictionaryCount
        ? t("settings.dictionary.count", { count: dictionaryCount })
        : t("settings.dictionary.noneImported"),
    },
  ];
}
