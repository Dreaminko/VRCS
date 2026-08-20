import assert from "node:assert/strict";
import test from "node:test";

import {
  recognitionEngineLabel,
  recognitionServicesForProfile,
  selectRecognitionService,
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

test("engine labels use catalog display names and safely fall back to service IDs", () => {
  const selected = selectRecognitionService(asr, definitions[0].services[0]);
  assert.equal(recognitionEngineLabel(selected, [profile], definitions), "Groq Transcription");
  assert.equal(recognitionEngineLabel(selected, [], []), "groq-transcribe");
});
