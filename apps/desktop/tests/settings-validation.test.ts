import { DEFAULT_FEATURE_SETTINGS } from "../src/settings/feature-availability.ts";
import test from "node:test";
import assert from "node:assert/strict";
import {
  audioSelectionErrors,
  hasEnabledAudioSource,
} from "../src/settings/settings-validation.ts";
import { DEFAULT_OCR_SETTINGS, DEFAULT_VR_OVERLAY_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import type { AudioDevice } from "../src/capture/types.ts";
import { DEFAULT_VISION_SETTINGS } from "../src/settings/vision-settings.ts";
import type { Settings } from "../src/settings/types.ts";

const settings: Settings = {
  schema_version: 42,
  model_defaults: {
    text: { profile_id: null, model: "" },
    translation: { profile_id: null, model: "" },
    vision: { profile_id: null, model: "" },
  },
  learning_ai: { profile_id: null, model: "", use_default_model: true, thinking_enabled: false },
  vrchat_chatbox: { enabled: false },
  conversation_titles: { auto_generate: false, profile_id: null, model: "", output_language: "auto" },
  features: { ...DEFAULT_FEATURE_SETTINGS },
  server: { host: "127.0.0.1", port: 8766 },
  storage: {
    database_path: "data/vrcs.db",
    model_directory: "models/asr",
    subtitle_history_max_bytes: 100 * 1024 * 1024,
    model_download_source: "auto",
  },
  audio: {
    sample_rate: 16_000,
    output: { mode: "system", device_id: 10, trigger_threshold_dbfs: -45 },
    microphone: { mode: "device", device_id: 20, trigger_threshold_dbfs: -45 },
  },
  vad: { endpointing: "silence", silence_seconds: 0.4, max_speech_seconds: 6 },
  asr: { backend: "qwen_local_managed", language: "auto", managed_qwen: { package_id: "qwen3-asr-0.6b-q8_0", device: "auto" }, active_profile_id: null, service_settings: {} },
  translation: { mode: "disabled", speaker_targets: [{ target_language: "zh-Hans", profile_id: null, model: "gpt-5-mini", thinking_enabled: false }], microphone_targets: [{ target_language: "en", profile_id: null, model: "gpt-5-mini", thinking_enabled: false }], prompt: { system_prompt: "", context_enabled: false, include_speaker: true, include_microphone: true, include_chatbox: true, max_messages: 5, max_chars: 4000 } },
  local_translation: { device: "cpu" },
  language_presets: [],
  glossary: { llm_enabled: true, asr_enabled: true, sources: [] },
  osc: { enabled: false, port: 9000, mute_sync_enabled: true, preserve_original_text: true, translation_strategy: "preferred_only" },
  dictionary: { selection_lookup_enabled: true },
  anki: { enabled: true, port: 8765, deck: "VRCS", model: "Basic", front_field: "Front", back_field: "Back" },
  external_api: { enabled: false, host: "127.0.0.1", port: 8767, require_token: false },
  vrcx: { enabled: false, port: 22500, include_in_llm_context: false, include_in_asr_context: false },
  vr_overlay: DEFAULT_VR_OVERLAY_SETTINGS,
  ocr: DEFAULT_OCR_SETTINGS,
  vision: DEFAULT_VISION_SETTINGS,
};

const devices: AudioDevice[] = [
  { id: 10, name: "output", is_default: true, is_loopback: true, sample_rate: 48_000, channels: 2 },
  { id: 20, name: "microphone", is_default: true, is_loopback: false, sample_rate: 48_000, channels: 1 },
];

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

test("foreign device keys never match a numeric device or the system default", () => {
  const imported: Settings = {
    ...settings,
    audio: {
      ...settings.audio,
      output: { ...settings.audio.output, device_id: null, device_key: "pipewire:sink:saved" },
      microphone: { ...settings.audio.microphone, device_key: "pipewire:source:saved" },
    },
  };
  assert.deepEqual(audioSelectionErrors(imported, devices), [
    "The selected system output device is no longer available",
    "The selected microphone device is no longer available",
  ]);
});

test("persistent keys take precedence over transient IDs and survive device refresh", () => {
  const keyed: Settings = {
    ...settings,
    audio: {
      ...settings.audio,
      output: { ...settings.audio.output, device_id: 999, device_key: "pipewire:sink:saved" },
      microphone: { ...settings.audio.microphone, device_id: null, device_key: "pipewire:source:saved" },
    },
  };
  const refreshed = devices.map((device) => ({
    ...device,
    device_key: device.is_loopback ? "pipewire:sink:saved" : "pipewire:source:saved",
  }));
  assert.equal(audioSelectionErrors(keyed, []).length, 2);
  assert.deepEqual(audioSelectionErrors(keyed, refreshed), []);
  assert.equal(keyed.audio.output.device_key, "pipewire:sink:saved");
  assert.equal(audioSelectionErrors(keyed, refreshed.map((device) => ({ ...device, is_loopback: !device.is_loopback }))).length, 2);
});

test("legacy numeric IDs do not match PipeWire nodes with coincident IDs", () => {
  const pipewire = devices.map((device) => ({ ...device, device_key: `pipewire:${device.id}` }));
  assert.equal(audioSelectionErrors(settings, pipewire).length, 2);
});

test("explicit keys on default microphones still require an exact source match", () => {
  const keyed: Settings = {
    ...settings,
    audio: { ...settings.audio, microphone: { ...settings.audio.microphone, mode: "default", device_id: null, device_key: "pipewire:source:saved" } },
  };
  assert.equal(audioSelectionErrors(keyed, devices).length, 1);
  assert.deepEqual(audioSelectionErrors(keyed, devices.map((device) => ({ ...device, device_key: device.is_loopback ? null : "pipewire:source:saved" }))), []);
});
