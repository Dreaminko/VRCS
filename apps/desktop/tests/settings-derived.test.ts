import { DEFAULT_FEATURE_SETTINGS } from "../src/settings/feature-availability.ts";
import test from "node:test";
import assert from "node:assert/strict";

import {
  createAnkiOptions,
  recognitionSourceValue,
  selectRecognitionSource,
} from "../src/settings/settings-derived.ts";
import { DEFAULT_OCR_SETTINGS, DEFAULT_VR_OVERLAY_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import type { ApiProfileView, ProviderDefinition } from "../src/providers/types.ts";
import type { AnkiStatus } from "../src/anki/types.ts";
import { DEFAULT_VISION_SETTINGS } from "../src/settings/vision-settings.ts";
import {
  applyModelDefaults,
  translationPromptBackend,
  translationSelectionReady,
} from "../src/settings/model-selection.ts";
import { targetLanguageCodes } from "../src/settings/translation/language-settings.ts";
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
    output: { mode: "system", device_id: null, trigger_threshold_dbfs: -45 },
    microphone: { mode: "default", device_id: null, trigger_threshold_dbfs: -45 },
  },
  vad: { endpointing: "silence", silence_seconds: 0.4, max_speech_seconds: 6 },
  asr: { backend: "qwen_local_managed", language: "auto", managed_qwen: { package_id: "qwen3-asr-0.6b-q8_0", device: "auto" }, active_profile_id: null, service_settings: {} },
  translation: { mode: "disabled", speaker_targets: [{ target_language: "zh-Hans", profile_id: null, model: "gpt-5-mini", thinking_enabled: false }], microphone_targets: [{ target_language: "en", profile_id: null, model: "gpt-5-mini", thinking_enabled: false }], prompt: { system_prompt: "", context_enabled: false, include_speaker: true, include_microphone: true, include_chatbox: true, max_messages: 5, max_chars: 4000 } },
  local_translation: { device: "cpu" },
  language_presets: [],
  glossary: { llm_enabled: true, asr_enabled: true, sources: [] },
  osc: { enabled: false, port: 9000, mute_sync_enabled: true, preserve_original_text: true, translation_strategy: "preferred_only" },
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
  ocr: DEFAULT_OCR_SETTINGS,
  vision: DEFAULT_VISION_SETTINGS,
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
    credential: { configured: true, stored_configured: true, environment_override: false, storage_available: true, reason_code: null, source: "credential_manager" },
    capabilities: {
      supports_streaming: false,
      supports_model_listing: false,
      requires_api_key: true,
      is_local: false,
      supports_context: false,
      supports_translation: enabled.includes("text_translation"),
      supports_asr: enabled.includes("speech_to_text"),
      supports_text_generation: enabled.includes("text_generation"),
      supports_image_understanding: enabled.includes("image_understanding"),
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
    capabilities: { supports_streaming: true, supports_model_listing: false, requires_api_key: true, is_local: false, supports_context: true, supports_translation: false, supports_asr: true, supports_text_generation: false, supports_image_understanding: false, supports_custom_translation_language: false, supported_languages: [] },
  },
];

const profiles = [
  profile("alpha-profile", "alpha", ["speech_to_text"]),
  profile("translation-profile", "alpha", ["text_translation"]),
];


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

  const local = selectRecognitionSource(selected, "managed_qwen", profiles, definitions);
  assert.equal(local.backend, "qwen_local_managed");
  assert.equal(local.active_profile_id, null);
  assert.equal(recognitionSourceValue(local), "managed_qwen");
});

test("translation readiness accepts local PLaMo with an API-capable fallback", () => {
  assert.equal(translationSelectionReady({
    backend: "plamo_local",
    profile_id: null,
    model: "plamo-3-translate-0.8b-q4_k_m",
  }, profiles), true);
  assert.equal(translationSelectionReady({
    backend: "plamo_local",
    profile_id: null,
    model: "another-model",
  }, profiles), false);
  assert.equal(translationSelectionReady({
    backend: "api",
    profile_id: "translation-profile",
    model: "api-model",
  }, profiles), true);
});

test("local translation defaults replace unsupported inherited target languages", () => {
  const local = applyModelDefaults({
    ...settings,
    translation: {
      ...settings.translation,
      model_selection: {
        backend: "plamo_local",
        profile_id: null,
        model: "plamo-3-translate-0.8b-q4_k_m",
        use_default_model: false,
      },
      speaker_targets: [{
        target_language: "pt-BR",
        profile_id: null,
        model: "",
        thinking_enabled: false,
        use_default_model: true,
      }],
    },
  });

  assert.equal(local.translation.speaker_targets[0].backend, "plamo_local");
  assert.equal(local.translation.speaker_targets[0].target_language, "ja");
});

test("live ASR keeps its first target language when local translation owns later routes", () => {
  const local = settings.translation.speaker_targets[0];
  const next = applyModelDefaults({
    ...settings,
    asr: { ...settings.asr, backend: "gemini_live_translate" },
    translation: {
      ...settings.translation,
      model_selection: {
        backend: "plamo_local",
        profile_id: null,
        model: "plamo-3-translate-0.8b-q4_k_m",
        use_default_model: false,
      },
      speaker_targets: [
        { ...local, target_language: "pt-BR", use_default_model: true },
        { ...local, target_language: "yue-Hant", use_default_model: true },
      ],
    },
  });

  assert.equal(next.translation.speaker_targets[0].target_language, "pt-BR");
  assert.equal(next.translation.speaker_targets[1].target_language, "ja");
});

test("live service language choices take priority over a local route backend", () => {
  const target = {
    target_language: "pt-BR",
    backend: "plamo_local" as const,
    profile_id: null,
    model: "plamo-3-translate-0.8b-q4_k_m",
    thinking_enabled: false,
  };

  assert.equal(
    targetLanguageCodes(target, [], profiles, "gemini_live_translate").includes("pt-BR"),
    true,
  );
});

test("API-only task bindings do not send a translation backend field", () => {
  const resolved = applyModelDefaults(settings);

  assert.equal("backend" in resolved.conversation_titles, false);
  assert.equal("backend" in resolved.vision, false);
  assert.equal("backend" in resolved.learning_ai, false);
  assert.equal(resolved.translation.model_selection?.backend, "api");
});

test("mixed translation routes keep the shared API prompt editor visible", () => {
  assert.equal(translationPromptBackend([
    { backend: "plamo_local" },
    { backend: "api" },
  ]), "api");
  assert.equal(translationPromptBackend([
    { backend: "plamo_local" },
    { backend: "plamo_local" },
  ]), "plamo_local");
});


test("Anki options retain current values and prevent duplicate field mapping", () => {
  const options = createAnkiOptions(ankiStatus, settings.anki);

  assert.deepEqual(options.decks, ["VRCS", "Default"]);
  assert.deepEqual(options.models.map((option) => option.value), ["Basic", "Japanese"]);
  assert.deepEqual(options.frontFields.map((option) => option.value), ["Front", "Reading"]);
  assert.deepEqual(options.backFields.map((option) => option.value), ["Back", "Reading"]);
});
