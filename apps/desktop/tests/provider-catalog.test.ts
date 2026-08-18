import assert from "node:assert/strict";
import test from "node:test";

import {
  defaultEnabledCapabilities,
  groupedProviderOptions,
  providerCapabilities,
} from "../src/provider-catalog.ts";
import type { ApiCapability, ProviderDefinition } from "../src/types.ts";

function definition(
  id: string,
  category: ProviderDefinition["category"],
  capabilities: ApiCapability[],
): ProviderDefinition {
  return {
    id,
    display_name: id.toUpperCase(),
    category,
    connection: {
      base_url: { mode: "fixed", default: null },
      auth_modes: ["bearer"],
      default_auth_mode: "bearer",
      fields: [],
    },
    services: [{
      id: `${id}-service`,
      display_name: `${id} service`,
      capabilities,
      adapter: id,
      recognition_transport: capabilities.includes("speech_to_text") ? "realtime_stream" : null,
      partial_results: true,
      models: [],
      model_listing: false,
      supports_context: false,
    }],
    support_levels: { asr: null, translation: null },
    capabilities: {
      supports_streaming: false,
      supports_model_listing: false,
      requires_api_key: true,
      is_local: false,
      supports_context: false,
      supports_translation: capabilities.includes("text_translation"),
      supports_asr: capabilities.includes("speech_to_text"),
      supports_text_generation: capabilities.includes("text_generation"),
      supports_custom_translation_language: false,
      supported_languages: [],
    },
  };
}

test("provider capabilities are derived from services", () => {
  const provider = definition("alpha", "cloud_provider", ["speech_to_text", "text_translation"]);
  assert.deepEqual(providerCapabilities(provider), ["speech_to_text", "text_translation"]);
});

test("Groq and custom protocol defaults follow the requested policy", () => {
  const allCapabilities: ApiCapability[] = ["speech_to_text", "text_generation", "text_translation"];
  assert.deepEqual(
    defaultEnabledCapabilities(definition("groq", "cloud_provider", allCapabilities)),
    ["text_generation", "text_translation", "speech_to_text"],
  );
  assert.deepEqual(
    defaultEnabledCapabilities(definition("custom", "custom_protocol", allCapabilities)),
    ["text_generation", "text_translation"],
  );
});

test("provider options are grouped and sorted by category", () => {
  const options = groupedProviderOptions([
    definition("custom", "custom_protocol", ["text_generation"]),
    definition("cloud", "cloud_provider", ["speech_to_text"]),
    definition("local", "local_service", ["speech_to_text"]),
  ], {
    cloud_provider: "Cloud",
    local_service: "Local",
    custom_protocol: "Custom",
  });

  assert.deepEqual(options.map(({ value, group }) => ({ value, group })), [
    { value: "cloud", group: "Cloud" },
    { value: "local", group: "Local" },
    { value: "custom", group: "Custom" },
  ]);
});
