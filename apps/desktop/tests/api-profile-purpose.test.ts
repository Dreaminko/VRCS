import assert from "node:assert/strict";
import test from "node:test";

import {
  apiProfilePurpose,
  supportsContext,
  supportsLlmModels,
  supportsRecognition,
  supportsTranslation,
} from "../src/api-profile-purpose.ts";
import type { ApiProfile, ApiProfileView } from "../src/types.ts";

const profile = (input: Partial<ApiProfile>): ApiProfile => ({
  id: "profile",
  name: "Profile",
  provider: "openai",
  ...input,
});

test("legacy API profiles infer their existing purpose", () => {
  const officialOpenAi = profile({});
  const compatibleOpenAi = profile({ provider: "openai_compatible", base_url: "https://api.deepseek.com/v1" });
  const deepL = profile({ provider: "deepl" });
  const gemini = profile({ provider: "gemini" });

  assert.equal(apiProfilePurpose(officialOpenAi), "shared");
  assert.equal(apiProfilePurpose(compatibleOpenAi), "llm");
  assert.equal(apiProfilePurpose(deepL), "llm");
  assert.equal(apiProfilePurpose(gemini), "llm");
});

const profileView = (
  input: Partial<ApiProfileView>,
  capabilities: Partial<ApiProfileView["capabilities"]> = {},
): ApiProfileView => ({
  ...profile(input),
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
    supported_languages: [],
    ...capabilities,
  },
  support_levels: { asr: null, translation: null },
  ...input,
});

test("profile views use backend capability data", () => {
  const view = profileView(
    { provider: "openai", purpose: "shared" },
    { supports_translation: true },
  );

  assert.equal(supportsRecognition(view), false);
  assert.equal(supportsTranslation(view), true);
  assert.equal(supportsLlmModels(view), false);
  assert.equal(supportsContext(view), false);
});

test("explicit API purposes isolate ASR and translation", () => {
  const asrOnly = profileView({ purpose: "asr" }, { supports_asr: true });
  const llmOnly = profileView({ purpose: "llm" }, { supports_translation: true });
  const compatible = profileView(
    { provider: "openai_compatible", purpose: "llm", base_url: "https://api.deepseek.com/v1" },
    { supports_translation: true, supports_model_listing: true, supports_context: true },
  );

  assert.equal(supportsRecognition(asrOnly), true);
  assert.equal(supportsTranslation(asrOnly), false);
  assert.equal(supportsLlmModels(asrOnly), false);
  assert.equal(supportsRecognition(llmOnly), false);
  assert.equal(supportsTranslation(llmOnly), true);
  assert.equal(supportsLlmModels(compatible), true);
  assert.equal(supportsContext(compatible), true);
  assert.equal(supportsRecognition(compatible), false);
});
