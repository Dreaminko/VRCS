import type { Settings } from "../types";

export type SettingsCategory =
  | "system"
  | "recognition"
  | "api"
  | "audio"
  | "dictionary"
  | "anki"
  | "debug";

export type SaveState = "idle" | "saving" | "saved" | "error";

export type ApplySettings = (
  update: (current: Settings) => Settings,
  afterSave?: () => void,
) => void;

export type SettingOption = {
  value: string;
  label: string;
};

export type DebugRow = {
  label: string;
  value: string;
};
