import type { Settings } from "../types";

export type SettingsCategory =
  | "system"
  | "recognition"
  | "translation"
  | "glossary"
  | "api"
  | "audio"
  | "learning"
  | "connections"
  | "vr_overlay"
  | "debug";

export type SaveState = "idle" | "saving" | "saved" | "error";

export type ApplySettings = (
  update: (current: Settings) => Settings,
  afterSave?: () => void,
  afterError?: () => void,
) => void;

export type SettingOption = {
  value: string;
  label: string;
};

export type DebugRow = {
  label: string;
  value: string;
};
