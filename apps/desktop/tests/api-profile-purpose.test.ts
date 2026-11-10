import assert from "node:assert/strict";
import test from "node:test";

import {
  supportsContext,
  supportsLlmModels,
  supportsRecognition,
  supportsTranslation,
} from "../src/providers/api-profile-purpose.ts";
import { apiProfileDraftErrors, createApiProfileDraft } from "../src/providers/api-profile-draft.ts";
import type { ApiCapability, ApiProfileView, ProviderDefinition } from "../src/providers/types.ts";

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
      storage_available: true,
      reason_code: null,
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
      supports_image_understanding: false,
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

test("provider capability flags cannot enable capabilities disabled in a profile", () => {
  const view = profileView([], {
    supports_asr: true,
    supports_translation: true,
    supports_text_generation: true,
    supports_model_listing: true,
  });

  assert.equal(supportsRecognition(view), false);
  assert.equal(supportsTranslation(view), false);
  assert.equal(supportsLlmModels(view), false);
});

test("translation and image profiles expose models without text generation", () => {
  for (const capability of ["text_translation", "image_understanding"] as const) {
    const view = profileView([capability], { supports_model_listing: true, supports_asr: true });
    assert.equal(supportsLlmModels(view), true);
    assert.equal(supportsRecognition(view), false);
  }
});

test("profile drafts explain unsafe URLs and invalid custom headers before saving", () => {
  const definition: ProviderDefinition = {
    id: "openai_compatible", display_name: "Compatible", category: "custom_protocol",
    connection: { base_url: { mode: "editable" }, auth_modes: ["bearer", "none"], default_auth_mode: "bearer", fields: [] },
    services: [], support_levels: { asr: null, translation: null },
    capabilities: profileView([]).capabilities,
  };
  const draft = {
    ...createApiProfileDraft([definition]), base_url: "https://example.com/v1",
    enabled_capabilities: ["text_translation"] as ApiCapability[],
    headers: [{ name: "X-Client", value: "VRCS" }],
  };
  const errors = (patch: Partial<typeof draft>) => apiProfileDraftErrors({ ...draft, ...patch }, definition, false, false);
  assert.deepEqual(errors({}), {});
  assert.equal(errors({ base_url: "http://example.com/v1" }).base_url, "httpsRequired");
  assert.equal(errors({ base_url: "https://example.com/v1?" }).base_url, "baseUrl");
  assert.deepEqual(errors({ base_url: "http://127.0.0.1:8000/v1" }), {});
  assert.equal(errors({ headers: [{ name: "Authorization", value: "secret" }] }).headers, "headerName");
  assert.equal(errors({ headers: [...draft.headers, { name: "x-client", value: "duplicate" }] }).headers, "headerDuplicate");
  assert.equal(errors({ headers: [{ name: "X-Client", value: "injected\r\nheader" }] }).headers, "headerValue");
});
