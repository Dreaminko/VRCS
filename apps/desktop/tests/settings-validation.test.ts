import test from "node:test";
import assert from "node:assert/strict";
import {
  asrSelectionError,
  audioSelectionErrors,
  hasEnabledAudioSource,
  validComputeTypes,
} from "../src/settings/settings-validation.ts";
import { DEFAULT_VR_OVERLAY_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import type { AsrCapabilities, AudioDevice, Settings } from "../src/types.ts";

const settings: Settings = {
  schema_version: 25,
  server: { host: "127.0.0.1", port: 8766 },
  storage: {
    database_path: "data/vrcs.db",
    model_directory: "models/whisper",
    subtitle_history_max_bytes: 100 * 1024 * 1024,
  },
  audio: {
    sample_rate: 16_000,
    output: { mode: "system", device_id: 10, trigger_threshold_dbfs: -45 },
    microphone: { mode: "device", device_id: 20, trigger_threshold_dbfs: -45 },
  },
  vad: { silence_seconds: 0.4, max_speech_seconds: 6 },
  asr: { backend: "local_whisper", language: "auto", local: { model: "small", device: "auto", compute_type: "int8" }, active_profile_id: null, service_settings: {}, cloud_failure_policy: "reconnect" },
  translation: { mode: "disabled", target_language: "zh-Hans", profile_id: null, model: "gpt-5-mini", thinking_enabled: false, microphone_target_language: "en", prompt: { system_prompt: "", context_enabled: false, include_speaker: true, include_microphone: true, include_chatbox: true, max_messages: 5, max_chars: 4000 } },
  glossary: { llm_enabled: true, asr_enabled: true, sources: [] },
  osc: { enabled: false, port: 9000, mute_sync_enabled: true, mute_status_toast_enabled: false, preserve_original_text: true },
  dictionary: { selection_lookup_enabled: true },
  anki: { enabled: true, port: 8765, deck: "VRCS", model: "Basic", front_field: "Front", back_field: "Back" },
  external_api: { enabled: false, host: "127.0.0.1", port: 8767, require_token: false },
  vrcx: { enabled: false, port: 22500, include_in_llm_context: false, include_in_asr_context: false },
  vr_overlay: DEFAULT_VR_OVERLAY_SETTINGS,
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

test("requires at least one enabled audio source", () => {
  assert.equal(hasEnabledAudioSource(settings), true);
  assert.equal(hasEnabledAudioSource({
    ...settings,
    audio: {
      ...settings.audio,
      output: { ...settings.audio.output, mode: "disabled", device_id: null },
      microphone: { ...settings.audio.microphone, mode: "disabled", device_id: null },
    },
  }), false);
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
