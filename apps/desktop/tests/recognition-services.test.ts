import assert from "node:assert/strict";
import test from "node:test";

import {
  liveTranslationServiceName,
  recognitionEngineLabel,
  recognitionServicesForProfile,
  selectRecognitionService,
  selectRecognitionProfile,
  updateRecognitionServiceSettings,
} from "../src/recognition-services.ts";
import type { ApiProfileView, AsrSettings, ProviderDefinition } from "../src/types.ts";

const profile: ApiProfileView = {
  id: "profile",
  name: "Profile",
  provider: "groq",
  enabled_capabilities: ["speech_to_text"],
  provider_display_name: "Groq",
  active: true,
  translation_active: false,
  credential: { configured: true, stored_configured: true, environment_override: false, source: "credential_manager" },
  capabilities: { supports_streaming: false, supports_model_listing: true, requires_api_key: true, is_local: false, supports_context: false, supports_translation: false, supports_asr: true, supports_text_generation: false, supports_custom_translation_language: false, supported_languages: [] },
  support_levels: { asr: "native", translation: null },
};

const definitions: ProviderDefinition[] = [{
  id: "groq",
  display_name: "Groq",
  category: "cloud_provider",
  connection: { base_url: { mode: "fixed", default: "https://api.groq.com" }, auth_modes: ["bearer"], default_auth_mode: "bearer", fields: [] },
  services: [{
    id: "groq-transcribe",
    display_name: "Groq Transcription",
    capabilities: ["speech_to_text"],
    adapter: "groq",
    recognition_transport: "segmented_upload",
    partial_results: false,
    models: ["whisper-large-v3-turbo"],
    model_listing: true,
    supports_context: false,
  }],
  support_levels: { asr: "native", translation: null },
  capabilities: profile.capabilities,
}];

const asr: AsrSettings = {
  backend: "local_whisper",
  language: "auto",
  local: { model: "small", device: "auto", compute_type: "int8" },
  active_profile_id: "profile",
  service_settings: {},
  cloud_failure_policy: "reconnect",
};

test("recognition services are selected from provider service metadata", () => {
  assert.deepEqual(
    recognitionServicesForProfile(profile, definitions).map((service) => service.id),
    ["groq-transcribe"],
  );
});

test("service selection initializes generic settings", () => {
  const selected = selectRecognitionService(asr, definitions[0].services[0]);
  assert.equal(selected.backend, "groq-transcribe");
  assert.deepEqual(selected.service_settings["groq-transcribe"], {
    model: "whisper-large-v3-turbo",
    context: "",
  });

  const updated = updateRecognitionServiceSettings(selected, "groq-transcribe", { context: "VRChat" });
  assert.equal(updated.service_settings["groq-transcribe"]?.context, "VRChat");
});

test("dynamic services preserve a discovered model outside the fallback list", () => {
  const service = { ...definitions[0].services[0], model_listing: true };
  const configured: AsrSettings = {
    ...asr,
    service_settings: {
      [service.id]: { model: "whisper-large-v3", context: "" },
    },
  };

  const selected = selectRecognitionService(configured, service);
  assert.equal(selected.service_settings[service.id]?.model, "whisper-large-v3");
});

test("services with preset models preserve a custom model name", () => {
  const service = { ...definitions[0].services[0], model_listing: false };
  const configured: AsrSettings = {
    ...asr,
    service_settings: {
      [service.id]: { model: "custom-transcribe-v1", context: "" },
    },
  };

  const selected = selectRecognitionService(configured, service);
  assert.equal(selected.service_settings[service.id]?.model, "custom-transcribe-v1");
});

test("engine labels use catalog display names and safely fall back to service IDs", () => {
  const selected = selectRecognitionService(asr, definitions[0].services[0]);
  assert.equal(recognitionEngineLabel(selected, [profile], definitions), "Groq Transcription");
  assert.equal(recognitionEngineLabel(selected, [], []), "groq-transcribe");
});

test("native translation services keep their own labels and model settings", () => {
  assert.equal(liveTranslationServiceName("openai_realtime"), undefined);
  assert.equal(liveTranslationServiceName("gemini_live_translate"), "Gemini Live Translate");
  assert.equal(liveTranslationServiceName("openai_realtime_translate"), "OpenAI Realtime Translation");
  const service = { ...definitions[0].services[0], id: "openai_realtime_translate", models: ["gpt-realtime-translate"] };
  const selected = selectRecognitionService(asr, service);
  assert.equal(selected.service_settings[service.id].model, "gpt-realtime-translate");
  assert.equal(selected.active_profile_id, asr.active_profile_id);
});

const liveDefinitions: ProviderDefinition[] = [
  {
    ...definitions[0], id: "openai", display_name: "OpenAI",
    services: [
      { ...definitions[0].services[0], id: "openai_realtime", models: ["gpt-4o-transcribe"], recognition_transport: "realtime_stream" },
      { ...definitions[0].services[0], id: "openai_realtime_translate", models: ["gpt-realtime-translate"], recognition_transport: "realtime_stream" },
    ],
  },
  {
    ...definitions[0], id: "gemini", display_name: "Gemini",
    services: [
      { ...definitions[0].services[0], id: "gemini_transcribe", models: ["gemini-3.5-transcribe-live"], recognition_transport: "realtime_stream" },
      { ...definitions[0].services[0], id: "gemini_live_translate", models: ["gemini-3.5-live-translate-preview"], recognition_transport: "realtime_stream" },
    ],
  },
];
const liveProfiles: ApiProfileView[] = [
  { ...profile, id: "oai", provider: "openai", provider_display_name: "OpenAI" },
  { ...profile, id: "gai", provider: "gemini", provider_display_name: "Gemini" },
];

test("switching OpenAI and Gemini profiles preserves native live translation in both directions", () => {
  const original: AsrSettings = {
    ...asr, active_profile_id: "oai", backend: "openai_realtime_translate",
    service_settings: { openai_realtime_translate: { model: "gpt-realtime-translate", context: "" } },
  };
  const gemini = selectRecognitionProfile(original, "gai", liveProfiles, liveDefinitions);
  assert.equal(gemini.active_profile_id, "gai");
  assert.equal(gemini.backend, "gemini_live_translate");
  assert.equal(gemini.service_settings.gemini_live_translate.model, "gemini-3.5-live-translate-preview");
  assert.equal(gemini.service_settings.openai_realtime_translate, original.service_settings.openai_realtime_translate);
  const restored = selectRecognitionProfile(gemini, "oai", liveProfiles, liveDefinitions);
  assert.equal(restored.active_profile_id, "oai");
  assert.equal(restored.backend, "openai_realtime_translate");
  assert.equal(restored.service_settings.openai_realtime_translate.model, "gpt-realtime-translate");
  assert.equal(original.backend, "openai_realtime_translate");
  assert.equal(original.active_profile_id, "oai");
});

test("live profile switches preserve customized models and prefer an exact service match", () => {
  const original: AsrSettings = {
    ...asr, active_profile_id: "oai", backend: "openai_realtime_translate",
    service_settings: { gemini_live_translate: { model: "user-live-model", context: "saved context" } },
  };
  const gemini = selectRecognitionProfile(original, "gai", liveProfiles, liveDefinitions);
  assert.equal(gemini.backend, "gemini_live_translate");
  assert.deepEqual(gemini.service_settings.gemini_live_translate, { model: "user-live-model", context: "saved context" });
  const secondGemini = { ...liveProfiles[1], id: "gai-two" };
  const switched = selectRecognitionProfile(gemini, "gai-two", [...liveProfiles, secondGemini], liveDefinitions);
  assert.equal(switched.active_profile_id, "gai-two");
  assert.equal(switched.backend, "gemini_live_translate");
});

test("ordinary recognition selection and providers without native translation retain their fallback", () => {
  const gemini = selectRecognitionProfile(
    { ...asr, backend: "openai_realtime", active_profile_id: "oai" }, "gai", liveProfiles, liveDefinitions,
  );
  assert.equal(gemini.backend, "gemini_transcribe");
  const groq = selectRecognitionProfile(
    { ...asr, backend: "openai_realtime_translate", active_profile_id: "oai" }, "profile", [...liveProfiles, profile], [...liveDefinitions, ...definitions],
  );
  assert.equal(groq.backend, "groq-transcribe");
  const local = selectRecognitionProfile(gemini, "local", liveProfiles, liveDefinitions);
  assert.equal(local.backend, "local_whisper");
  assert.equal(local.active_profile_id, null);
});
