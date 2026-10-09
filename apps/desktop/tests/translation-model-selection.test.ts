import assert from "node:assert/strict";
import test from "node:test";

import {
  selectTranslationModel,
  switchTranslationProfile,
  updateTranslationModel,
  translationDiagnosticModel,
} from "../src/translation-model-selection.ts";
import type { ApiProfileView } from "../src/types.ts";
import type { TranslationTargetSettings } from "../src/settings/types.ts";

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
    selectTranslationModel(
      "openai",
      ["text-embedding-3-small", "gpt-5-mini", "gpt-4.1-mini"],
      "text-embedding-3-small",
    ),
    "gpt-4.1-mini",
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
  assert.equal(
    selectTranslationModel(
      "alibaba_token_plan",
      ["qwen-audio-3.0-asr-flash", "qwen3.7-plus", "qwen3.6-flash"],
      "",
    ),
    "qwen3.6-flash",
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

test("switching away and back restores each profile's last model after saving", () => {
  const original: TranslationTargetSettings = {
    target_language: "ja", profile_id: "deepseek", model: "deepseek-v4-pro", thinking_enabled: false,
  };
  let target = switchTranslationProfile(original, profile("openai"), ["gpt-4.1-mini", "gpt-5-mini"]);
  assert.equal(target.model, "gpt-4.1-mini");
  target = updateTranslationModel(target, "gpt-5-mini");
  target = JSON.parse(JSON.stringify(target));
  target = switchTranslationProfile(target, profile("deepseek"), ["deepseek-v4-flash", "deepseek-v4-pro"]);
  assert.equal(target.model, "deepseek-v4-pro");
  target = switchTranslationProfile(target, profile("openai"), ["gpt-4.1-mini", "gpt-5-mini"]);
  assert.equal(target.model, "gpt-5-mini");
  assert.equal(original.model, "deepseek-v4-pro");
  assert.equal(original.model_by_profile, undefined);
});

test("manual models survive unavailable catalogs and profiles of the same provider stay separate", () => {
  const first = { ...profile("openai_compatible"), id: "custom-one" };
  const second = { ...first, id: "custom-two" };
  let target: TranslationTargetSettings = {
    target_language: "en", profile_id: first.id, model: "manual-one", thinking_enabled: false,
  };
  target = switchTranslationProfile(target, second);
  target = updateTranslationModel(target, "manual-two");
  target = switchTranslationProfile(target, first);
  assert.equal(target.model, "manual-one");
  target = switchTranslationProfile(target, second);
  assert.equal(target.model, "manual-two");
});

test("translation routes keep independent model choices for the same profile", () => {
  const first: TranslationTargetSettings = {
    target_language: "en", profile_id: "deepseek", model: "deepseek-v4-pro", thinking_enabled: false,
  };
  const second = { ...first, model: "deepseek-v4-flash" };
  const next = profile("openai");
  const firstAway = switchTranslationProfile(first, next, ["gpt-5-mini"]);
  const secondAway = switchTranslationProfile(second, next, ["gpt-5-mini"]);
  assert.equal(switchTranslationProfile(firstAway, profile("deepseek")).model, "deepseek-v4-pro");
  assert.equal(switchTranslationProfile(secondAway, profile("deepseek")).model, "deepseek-v4-flash");
});
