import test from "node:test";
import assert from "node:assert/strict";
import type { TFunction } from "i18next";

import {
  classifyModels,
  createAnkiOptions,
  createSettingsActionState,
} from "../src/settings/settings-derived.ts";
import type {
  AsrCapabilities,
  AsrModelRecord,
  AnkiStatus,
  Settings,
} from "../src/types.ts";

const t = ((key: string) => key) as TFunction;

const settings: Settings = {
  schema_version: 3,
  server: { host: "127.0.0.1", port: 8766 },
  storage: {
    database_path: "data/vrcs.db",
    model_directory: "models/whisper",
    subtitle_history_limit: 500,
  },
  audio: {
    sample_rate: 16_000,
    output: { mode: "system", device_id: null },
    microphone: { mode: "default", device_id: null },
  },
  vad: { silence_seconds: 0.4, max_speech_seconds: 6 },
  asr: { model: "small", language: "auto", device: "auto", compute_type: "int8" },
  anki: {
    port: 8765,
    deck: "VRCS",
    model: "Basic",
    front_field: "Front",
    back_field: "Back",
  },
};

const ankiStatus: AnkiStatus = {
  connected: true,
  version: 6,
  decks: ["Default", "VRCS"],
  models: ["Basic", "Japanese"],
  fields: ["Front", "Back", "Reading"],
  configuration_valid: true,
  error_code: null,
  status_code: "ready",
  params: {},
  detail: "",
  message: "",
};

test("settings action state preserves category-specific priority", () => {
  const base = {
    saveState: "idle" as const,
    saveMessage: "",
    validationError: null,
    ankiPortError: "",
    ankiStatus: null,
    ankiMessage: "",
    desktopReady: true,
    desktopSaveState: "idle" as const,
    desktopMessage: "",
    t,
  };

  assert.deepEqual(
    createSettingsActionState({
      ...base,
      activeCategory: "dictionary",
    }),
    { text: "settings.action.dictionaryImmediate", state: "idle" },
  );
  assert.deepEqual(
    createSettingsActionState({
      ...base,
      activeCategory: "system",
      desktopReady: false,
    }),
    { text: "settings.action.readingDesktop", state: "idle" },
  );
  assert.deepEqual(
    createSettingsActionState({
      ...base,
      activeCategory: "anki",
      ankiPortError: "invalid port",
    }),
    { text: "invalid port", state: "error" },
  );
  assert.deepEqual(
    createSettingsActionState({
      ...base,
      activeCategory: "audio",
      validationError: "missing device",
    }),
    { text: "missing device", state: "idle" },
  );
});

test("model classification keeps the current model selectable", () => {
  const capabilities: AsrCapabilities = {
    runtime_available: true,
    cuda: {
      available: false,
      device_count: 0,
      devices: [],
      error: null,
    },
    models: [
      { id: "small", status: "not_downloaded" },
      { id: "base", status: "ready" },
    ],
    compute_types: {
      auto: ["int8"],
      cpu: ["int8"],
      cuda: [],
    },
  };
  const managed: AsrModelRecord[] = [];
  const result = classifyModels(managed, capabilities, "small", false);

  assert.deepEqual(result.selectable.map((model) => model.id), ["small", "base"]);
  assert.deepEqual(result.installed, []);
  assert.deepEqual(result.downloading, []);
});

test("Anki options retain current values and prevent duplicate field mapping", () => {
  const options = createAnkiOptions(ankiStatus, settings.anki);

  assert.deepEqual(options.decks, ["VRCS", "Default"]);
  assert.deepEqual(options.models.map((option) => option.value), ["Basic", "Japanese"]);
  assert.deepEqual(options.frontFields.map((option) => option.value), ["Front", "Reading"]);
  assert.deepEqual(options.backFields.map((option) => option.value), ["Back", "Reading"]);
});
