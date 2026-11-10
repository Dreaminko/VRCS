import assert from "node:assert/strict";
import test from "node:test";

import {
  defaultEnabledCapabilities,
  groupedProviderOptions,
  profileEnabledCapabilities,
  profileSupportsCapability,
  providerCapabilities,
} from "../src/providers/provider-catalog.ts";
import type { ApiCapability, ApiProfileView, ProviderDefinition } from "../src/providers/types.ts";

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
      supports_image_understanding: capabilities.includes("image_understanding"),
      supports_custom_translation_language: false,
      supported_languages: [],
    },
  };
}

function profile(
  enabledCapabilities: ApiCapability[],
  supportedCapabilities: ApiCapability[],
): ApiProfileView {
  return {
    id: "profile",
    name: "Profile",
    provider: "provider",
    enabled_capabilities: enabledCapabilities,
    provider_display_name: "Provider",
    active: false,
    translation_active: false,
    credential: { configured: false, stored_configured: false, environment_override: false, storage_available: true, reason_code: null, source: null },
    capabilities: definition("provider", "cloud_provider", supportedCapabilities).capabilities,
    support_levels: { asr: null, translation: null },
  };
}

test("empty enabled capabilities do not inherit provider flags", () => {
  const allCapabilities: ApiCapability[] = [
    "text_generation", "text_translation", "speech_to_text", "image_understanding",
  ];
  const view = profile([], allCapabilities);
  assert.deepEqual(profileEnabledCapabilities(view), []);
  for (const capability of allCapabilities) {
    assert.equal(profileSupportsCapability(view, capability), false, capability);
  }
  assert.deepEqual(profileEnabledCapabilities(profile([], [])), []);
});

test("explicit enabled capabilities are preserved without merging compatibility flags", () => {
  const view = profile(["text_generation"], ["text_generation", "image_understanding"]);
  assert.equal(profileEnabledCapabilities(view), view.enabled_capabilities);
  assert.deepEqual(profileEnabledCapabilities(view), ["text_generation"]);
});

test("image and text support follow explicit capabilities independently", () => {
  const textOnly = profile(["text_generation"], ["text_generation"]);
  assert.equal(profileSupportsCapability(textOnly, "text_generation"), true);
  assert.equal(profileSupportsCapability(textOnly, "image_understanding"), false);

  const imageOnly = profile(["image_understanding"], ["image_understanding"]);
  assert.equal(profileSupportsCapability(imageOnly, "image_understanding"), true);
  assert.equal(profileSupportsCapability(imageOnly, "text_generation"), false);

  assert.equal(profileSupportsCapability(profile(["image_understanding"], []), "image_understanding"), true);
  assert.equal(profileSupportsCapability(profile(["text_generation"], ["image_understanding"]), "image_understanding"), false);
});

test("provider capability flags do not enable capabilities omitted from the enabled list", () => {
  const view = profile(["image_understanding"], ["text_generation", "text_translation", "speech_to_text"]);
  for (const capability of ["text_generation", "text_translation", "speech_to_text"] as const) {
    assert.equal(profileSupportsCapability(view, capability), false, capability);
  }
  assert.equal(profileSupportsCapability(view, "image_understanding"), true);
});

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
