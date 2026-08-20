import test from "node:test";
import assert from "node:assert/strict";

import {
  classifyModels,
  createAnkiOptions,
  recognitionSourceValue,
  selectRecognitionSource,
  showsLocalRecognitionSettings,
} from "../src/settings/settings-derived.ts";
import { DEFAULT_VR_OVERLAY_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import type {
  ApiProfileView,
  AsrCapabilities,
  AsrModelRecord,
  AnkiStatus,
  ProviderDefinition,
  Settings,
} from "../src/types.ts";

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
    output: { mode: "system", device_id: null, trigger_threshold_dbfs: -45 },
    microphone: { mode: "default", device_id: null, trigger_threshold_dbfs: -45 },
  },
  vad: { silence_seconds: 0.4, max_speech_seconds: 6 },
  asr: { backend: "local_whisper", language: "auto", local: { model: "small", device: "auto", compute_type: "int8" }, active_profile_id: null, service_settings: {}, cloud_failure_policy: "reconnect" },
  translation: { mode: "disabled", target_language: "zh-Hans", profile_id: null, model: "gpt-5-mini", thinking_enabled: false, microphone_target_language: "en", prompt: { system_prompt: "", context_enabled: false, include_speaker: true, include_microphone: true, include_chatbox: true, max_messages: 5, max_chars: 4000 } },
  glossary: { llm_enabled: true, asr_enabled: true, sources: [] },
  osc: { enabled: false, port: 9000, mute_sync_enabled: true, mute_status_toast_enabled: false, preserve_original_text: true },
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
  vr_overlay: DEFAULT_VR_OVERLAY_SETTINGS,
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

function profile(id: string, provider: string, enabled: ApiProfileView["enabled_capabilities"]): ApiProfileView {
  return {
    id,
    name: id,
    provider,
    enabled_capabilities: enabled,
    provider_display_name: provider,
    active: false,
    translation_active: false,
    credential: { configured: true, stored_configured: true, environment_override: false, source: "credential_manager" },
    capabilities: {
      supports_streaming: false,
      supports_model_listing: false,
      requires_api_key: true,
      is_local: false,
      supports_context: false,
      supports_translation: enabled.includes("text_translation"),
      supports_asr: enabled.includes("speech_to_text"),
      supports_text_generation: enabled.includes("text_generation"),
      supports_custom_translation_language: false,
      supported_languages: [],
    },
    support_levels: { asr: null, translation: null },
  };
}

const definitions: ProviderDefinition[] = [
  {
    id: "alpha",
    display_name: "Alpha",
    category: "cloud_provider",
    connection: { base_url: { mode: "fixed", default: "https://alpha.example" }, auth_modes: ["bearer"], default_auth_mode: "bearer", fields: [] },
    services: [
      { id: "alpha-live", display_name: "Alpha Live", capabilities: ["speech_to_text"], adapter: "alpha", recognition_transport: "realtime_stream", partial_results: true, models: ["alpha-1"], model_listing: false, supports_context: true },
      { id: "alpha-batch", display_name: "Alpha Batch", capabilities: ["speech_to_text"], adapter: "alpha", recognition_transport: "segmented_upload", partial_results: false, models: ["alpha-2"], model_listing: false, supports_context: false },
    ],
    support_levels: { asr: "native", translation: null },
    capabilities: { supports_streaming: true, supports_model_listing: false, requires_api_key: true, is_local: false, supports_context: true, supports_translation: false, supports_asr: true, supports_text_generation: false, supports_custom_translation_language: false, supported_languages: [] },
  },
];

const profiles = [
  profile("alpha-profile", "alpha", ["speech_to_text"]),
  profile("translation-profile", "alpha", ["text_translation"]),
];

test("only local ASR shows local recognition settings", () => {
  assert.equal(showsLocalRecognitionSettings("local_whisper"), true);
  assert.equal(showsLocalRecognitionSettings("alpha-live"), false);
});

test("recognition source selects a profile and service atomically", () => {
  const selected = selectRecognitionSource(settings.asr, "alpha-profile", profiles, definitions);
  assert.equal(selected.active_profile_id, "alpha-profile");
  assert.equal(selected.backend, "alpha-live");
  assert.equal(selected.service_settings["alpha-live"]?.model, "alpha-1");
  assert.equal(recognitionSourceValue(selected), "alpha-profile");

  assert.deepEqual(
    selectRecognitionSource(selected, "translation-profile", profiles, definitions),
    selected,
  );

  const local = selectRecognitionSource(selected, "local", profiles, definitions);
  assert.equal(local.backend, "local_whisper");
  assert.equal(local.active_profile_id, null);
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
