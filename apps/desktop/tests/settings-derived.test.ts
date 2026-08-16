import test from "node:test";
import assert from "node:assert/strict";

import {
  classifyModels,
  createAnkiOptions,
  recognitionSourceValue,
  selectRecognitionSource,
  showsLocalRecognitionSettings,
} from "../src/settings/settings-derived.ts";
import type {
  AsrCapabilities,
  AsrModelRecord,
  AnkiStatus,
  Settings,
} from "../src/types.ts";

const settings: Settings = {
  schema_version: 21,
  server: { host: "127.0.0.1", port: 8766 },
  storage: {
    database_path: "data/vrcs.db",
    model_directory: "models/whisper",
    subtitle_history_max_bytes: 100 * 1024 * 1024,
  },
  audio: {
    sample_rate: 16_000,
    output: { mode: "system", device_id: null },
    microphone: { mode: "default", device_id: null, trigger_threshold_dbfs: -45 },
  },
  vad: { silence_seconds: 0.4, max_speech_seconds: 6 },
  asr: { backend: "local_whisper", language: "auto", local: { model: "small", device: "auto", compute_type: "int8" }, qwen: { context: "", model: "qwen3-asr-flash-realtime" }, fun_asr: { context: "", model: "fun-asr-realtime" }, openai: { model: "gpt-4o-mini-transcribe" }, api_profiles: [], active_api_profiles: { alibaba_cloud: null, openai: null }, cloud_failure_policy: "reconnect" },
  translation: { mode: "disabled", target_language: "zh-Hans", profile_id: null, model: "gpt-5-mini", thinking_enabled: false, microphone_target_language: "en", prompt: { system_prompt: "", context_enabled: false, include_speaker: true, include_microphone: true, include_chatbox: true, max_messages: 5, max_chars: 4000, glossary_sources: [] } },
  osc: { enabled: false, port: 9000, mute_sync_enabled: true, mute_status_toast_enabled: false },
  dictionary: { selection_lookup_enabled: true },
  anki: {
    enabled: true,
    port: 8765,
    deck: "VRCS",
    model: "Basic",
    front_field: "Front",
    back_field: "Back",
  },
  external_api: { enabled: false, host: "127.0.0.1", port: 8767, require_token: false },
  vrcx: { enabled: false, port: 22500, include_in_llm_context: false, include_in_asr_context: false },
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

test("only local ASR shows local recognition settings", () => {
  assert.equal(showsLocalRecognitionSettings("local_whisper"), true);
  assert.equal(showsLocalRecognitionSettings("qwen_realtime"), false);
  assert.equal(showsLocalRecognitionSettings("fun_asr_realtime"), false);
  assert.equal(showsLocalRecognitionSettings("openai_realtime"), false);
});

test("recognition source selects a named API profile and compatible backend atomically", () => {
  const withProfiles: Settings["asr"] = {
    ...settings.asr,
    api_profiles: [
      { id: "ali-work", name: "Work", provider: "alibaba_cloud", region: "singapore" },
      { id: "openai-personal", name: "Personal", provider: "openai", purpose: "asr" },
      { id: "openai-llm", name: "OpenAI LLM", provider: "openai", purpose: "llm" },
      { id: "deepseek", name: "DeepSeek", provider: "openai_compatible", base_url: "https://api.deepseek.com/v1" },
    ],
  };

  const openai = selectRecognitionSource(withProfiles, "openai-personal");
  assert.equal(openai.backend, "openai_realtime");
  assert.equal(openai.active_api_profiles.openai, "openai-personal");
  assert.equal(recognitionSourceValue(openai), "openai-personal");

  assert.deepEqual(selectRecognitionSource(openai, "openai-llm"), openai);
  assert.deepEqual(selectRecognitionSource(openai, "deepseek"), openai);

  const alibaba = selectRecognitionSource({ ...openai, backend: "fun_asr_realtime" }, "ali-work");
  assert.equal(alibaba.backend, "fun_asr_realtime");
  assert.equal(alibaba.active_api_profiles.alibaba_cloud, "ali-work");
  assert.equal(recognitionSourceValue(alibaba), "ali-work");

  const local = selectRecognitionSource(alibaba, "local");
  assert.equal(local.backend, "local_whisper");
  assert.equal(recognitionSourceValue(local), "local");
});

test("model classification keeps the current model selectable", () => {
  const capabilities: AsrCapabilities = {
    runtime_available: true,
    cuda: {
      available: false,
      device_count: 0,
      error: null,
    },
    models: [
      { id: "small", repository: "ggerganov/whisper.cpp", status: "not_downloaded" },
      { id: "base", repository: "ggerganov/whisper.cpp", status: "ready" },
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
