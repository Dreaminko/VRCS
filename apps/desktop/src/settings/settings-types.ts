import type { Settings } from "./types";
import type { QwenModelRecord, QwenRuntimeStatus } from "../providers/types";

export type SettingsCategory =
  | "system"
  | "recognition"
  | "translation"
  | "glossary"
  | "api"
  | "audio"
  | "learning"
  | "connections"
  | "about"
  | "ocr"
  | "vision"
  | "vr_overlay"
  | "debug";

export type SaveState = "idle" | "saving" | "saved" | "error";

export type ApplySettings = (
  update: (current: Settings) => Settings,
  afterSave?: () => void,
  afterError?: () => void,
) => void;

export type RecognitionModels = {
  directoryText: string;
  qwen: QwenModelRecord[];
  qwenReady: boolean;
  qwenRuntime: QwenRuntimeStatus | null;
  qwenMessage: string;
};

export type RecognitionActions = {
  updateAsr: <K extends keyof Settings["asr"]>(key: K, value: Settings["asr"][K]) => void;
  updateRecognitionSource: (source: string) => void;
  updateRecognitionService: (serviceId: string) => void;
  updateManagedQwen: <K extends keyof Settings["asr"]["managed_qwen"]>(
    key: K,
    value: Settings["asr"]["managed_qwen"][K],
  ) => void;
  updateVad: <K extends keyof Settings["vad"]>(key: K, value: Settings["vad"][K]) => void;
  setModelDirectoryText: (value: string) => void;
  updateModelDirectory: (value: string) => void;
  chooseModelDirectory: () => Promise<void>;
  loadQwenModels: () => Promise<void>;
  downloadQwenModel: (model: QwenModelRecord) => Promise<void>;
  downloadQwenRuntime: () => Promise<void>;
  cancelQwenRuntimeDownload: () => Promise<void>;
  cancelQwenDownload: (model: QwenModelRecord) => Promise<void>;
  verifyQwenModel: (model: QwenModelRecord) => Promise<void>;
  removeQwenModel: (model: QwenModelRecord) => Promise<void>;
};

export type SettingOption = {
  value: string;
  label: string;
};

export type DebugRow = {
  label: string;
  value: string;
};
