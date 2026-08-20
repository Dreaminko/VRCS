import assert from "node:assert/strict";
import test from "node:test";

import {
  selectTranslationModel,
  translationDiagnosticModel,
} from "../src/translation-model-selection.ts";
import type { ApiProfileView } from "../src/types.ts";

function profile(provider: string, supportsModelListing = true): ApiProfileView {
  return {
    id: provider,
    name: provider,
    provider,
    enabled_capabilities: ["text_generation", "text_translation"],
    provider_display_name: provider,
    active: false,
    translation_active: true,
    credential: {
      configured: true,
      stored_configured: true,
      environment_override: false,
      source: "credential_manager",
    },
    capabilities: {
      supports_streaming: true,
      supports_model_listing: supportsModelListing,
      requires_api_key: true,
      is_local: false,
      supports_context: true,
      supports_translation: true,
      supports_asr: false,
      supports_text_generation: true,
      supports_custom_translation_language: true,
      supported_languages: [],
    },
    support_levels: { asr: null, translation: "native" },
  };
}

test("profile switching reuses a valid model and otherwise prefers the provider default", () => {
  assert.equal(
    selectTranslationModel("deepseek", ["deepseek-v4-pro", "deepseek-v4-flash"], "deepseek-v4-pro"),
    "deepseek-v4-pro",
  );
  assert.equal(
    selectTranslationModel("deepseek", ["deepseek-v4-pro", "deepseek-v4-flash"], "gpt-5-mini"),
    "deepseek-v4-flash",
  );
});

test("automatic selection skips obvious non-text models", () => {
  assert.equal(
    selectTranslationModel("openai", ["text-embedding-3-small", "gpt-5-mini"], "text-embedding-3-small"),
    "gpt-5-mini",
  );
  assert.equal(
    selectTranslationModel("groq", ["whisper-large-v3", "openai/gpt-oss-120b"], ""),
    "openai/gpt-oss-120b",
  );
  assert.equal(
    selectTranslationModel("gemini", ["text-embedding-004", "gemini-2.5-flash"], ""),
    "gemini-2.5-flash",
  );
  assert.equal(
    selectTranslationModel("alibaba_cloud", ["text-embedding-v4", "qwen3.8-max"], ""),
    "qwen3.8-max",
  );
});

test("custom profiles keep manual model control when a catalog is unavailable", () => {
  const custom = profile("openai_compatible");
  assert.equal(translationDiagnosticModel(custom, undefined, "manual-model"), "manual-model");
  assert.equal(selectTranslationModel(custom.provider, ["embedding-only"], "manual-model"), undefined);
});

test("successful model catalogs only pass models they contain to diagnostics", () => {
  const deepseek = profile("deepseek");
  assert.equal(
    translationDiagnosticModel(deepseek, { models: ["deepseek-v4-flash"], loading: false, error: "" }, "gpt-5-mini"),
    undefined,
  );
  assert.equal(
    translationDiagnosticModel(deepseek, { models: ["deepseek-v4-flash"], loading: false, error: "" }, "deepseek-v4-flash"),
    "deepseek-v4-flash",
  );
});
