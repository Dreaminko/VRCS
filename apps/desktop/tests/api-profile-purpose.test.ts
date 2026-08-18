import assert from "node:assert/strict";
import test from "node:test";

import {
  supportsContext,
  supportsLlmModels,
  supportsRecognition,
  supportsTranslation,
} from "../src/api-profile-purpose.ts";
import type { ApiCapability, ApiProfileView } from "../src/types.ts";

function profileView(
  enabledCapabilities: ApiCapability[],
  capabilities: Partial<ApiProfileView["capabilities"]> = {},
): ApiProfileView {
  return {
    id: "profile",
    name: "Profile",
    provider: "provider",
    enabled_capabilities: enabledCapabilities,
    provider_display_name: "Provider",
    active: false,
    translation_active: false,
    credential: {
      configured: false,
      stored_configured: false,
      environment_override: false,
      source: null,
    },
    capabilities: {
      supports_streaming: false,
      supports_model_listing: false,
      requires_api_key: true,
      is_local: false,
      supports_context: false,
      supports_translation: false,
      supports_asr: false,
      supports_text_generation: false,
      supports_custom_translation_language: false,
      supported_languages: [],
      ...capabilities,
    },
    support_levels: { asr: null, translation: null },
  };
}

test("profile helpers use enabled capabilities", () => {
  const view = profileView(
    ["speech_to_text", "text_generation", "text_translation"],
    { supports_model_listing: true, supports_context: true },
  );

  assert.equal(supportsRecognition(view), true);
  assert.equal(supportsTranslation(view), true);
  assert.equal(supportsLlmModels(view), true);
  assert.equal(supportsContext(view), true);
});

test("compatibility capability fields remain supported for older profile views", () => {
  const view = profileView([], {
    supports_asr: true,
    supports_translation: true,
    supports_text_generation: true,
    supports_model_listing: true,
  });

  assert.equal(supportsRecognition(view), true);
  assert.equal(supportsTranslation(view), true);
  assert.equal(supportsLlmModels(view), true);
});
