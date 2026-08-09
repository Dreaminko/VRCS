import test from "node:test";
import assert from "node:assert/strict";
import {
  asrSelectionError,
  audioSettingsChanged,
  audioSelectionErrors,
  validComputeTypes,
} from "../src/settings-validation.ts";
import type { AsrCapabilities, AudioDevice, Settings } from "../src/types.ts";

const settings: Settings = {
  schema_version: 7,
  server: { host: "127.0.0.1", port: 8766 },
  storage: {
    database_path: "data/vrcs.db",
    model_directory: "models/whisper",
    subtitle_history_limit: 500,
  },
  audio: {
    sample_rate: 16_000,
    output: { mode: "system", device_id: 10 },
    microphone: { mode: "device", device_id: 20 },
  },
  vad: { silence_seconds: 0.4, max_speech_seconds: 6 },
  asr: { backend: "local_whisper", language: "auto", local: { model: "small", device: "auto", compute_type: "int8" }, qwen: { context: "", model: "qwen3-asr-flash-realtime" }, fun_asr: { context: "", model: "fun-asr-realtime" }, openai: { model: "gpt-4o-mini-transcribe" }, api_profiles: [], active_api_profiles: { alibaba_cloud: null, openai: null }, cloud_failure_policy: "reconnect" },
  translation: { mode: "disabled", target_language: "zh-Hans", profile_id: null, model: "gpt-5-mini", thinking_enabled: false },
  dictionary: { selection_lookup_enabled: true },
  anki: { enabled: true, port: 8765, deck: "VRCS", model: "Basic", front_field: "Front", back_field: "Back" },
};

const devices: AudioDevice[] = [
  { id: 10, name: "output", is_default: true, is_loopback: true, sample_rate: 48_000, channels: 2 },
  { id: 20, name: "microphone", is_default: true, is_loopback: false, sample_rate: 48_000, channels: 1 },
];

const capabilities: AsrCapabilities = {
  runtime_available: true,
  cuda: { available: false, device_count: 0, error: null },
  compute_types: { auto: ["int8"], cpu: ["int8"], cuda: [] },
  models: [],
};

test("accepts available output and microphone selections", () => {
  assert.deepEqual(audioSelectionErrors(settings, devices), []);
});

test("reports stale output and microphone selections independently", () => {
  assert.deepEqual(audioSelectionErrors(settings, []), [
    "The selected system output device is no longer available",
    "The selected microphone device is no longer available",
  ]);
});

test("filters compute types and rejects unavailable CUDA", () => {
  assert.deepEqual(validComputeTypes(capabilities, "cpu"), ["int8"]);
  assert.equal(
    asrSelectionError(
      { ...settings, asr: { ...settings.asr, local: { ...settings.asr.local, device: "cuda" } } },
      capabilities,
    ),
    "CUDA preflight failed; use automatic selection or CPU",
  );
});

test("audio and recognition changes require a running capture restart", () => {
  assert.equal(
    audioSettingsChanged(
      settings,
      {
        ...settings,
        audio: {
          ...settings.audio,
          output: { mode: "vrchat", device_id: null },
        },
      },
    ),
    true,
  );
  assert.equal(
    audioSettingsChanged(
      settings,
      { ...settings, asr: { ...settings.asr, local: { ...settings.asr.local, model: "base" } } },
    ),
    true,
  );
});
