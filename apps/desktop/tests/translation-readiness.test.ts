import assert from "node:assert/strict";
import test from "node:test";

import type { ApiProfileView } from "../src/providers/types.ts";
import {
  errorRequiresTranslationModelSettings,
  liveTranslationOnlyNeedsModelForManualOrExtraTargets,
  translationModelIssue,
} from "../src/settings/translation/translation-readiness.ts";
import type { Settings } from "../src/settings/types.ts";

function makeProfile(overrides: Partial<ApiProfileView> = {}): ApiProfileView {
  return {
    id: "openai-1",
    name: "OpenAI",
    provider: "openai",
    enabled_capabilities: ["text_translation"],
    capabilities: {
      supports_translation: true,
      supports_model_listing: true,
      supports_text_generation: true,
      supports_image_understanding: false,
      supports_speech_to_text: false,
      supports_custom_translation_language: true,
      supported_languages: ["en", "ja"],
      is_local: false,
    },
    ...overrides,
  } as unknown as ApiProfileView;
}

function makeSettings(): Settings {
  return {
    asr: { backend: "qwen_managed" },
    model_defaults: {
      text: { backend: "api", profile_id: null, model: "" },
      translation: { backend: "api", profile_id: null, model: "" },
      vision: { backend: "api", profile_id: null, model: "" },
    },
    translation: {
      mode: "disabled",
      model_selection: { backend: "api", profile_id: null, model: "", use_default_model: true },
      speaker_targets: [{ backend: "api", profile_id: null, model: "", use_default_model: true, target_language: "en" }],
      microphone_targets: [{ backend: "api", profile_id: null, model: "", use_default_model: true, target_language: "ja" }],
    },
  } as unknown as Settings;
}

test("detects each translation model readiness issue accurately", () => {
  const settings = makeSettings();
  assert.equal(translationModelIssue(settings, []), "no_profiles");

  const llmProfile = makeProfile();
  assert.equal(translationModelIssue(settings, [llmProfile]), "default_model_missing");

  settings.translation.model_selection = {
    backend: "api",
    profile_id: null,
    model: "",
    use_default_model: false,
  };
  assert.equal(translationModelIssue(settings, [llmProfile]), "profile_missing");

  settings.translation.model_selection.profile_id = "missing-id";
  assert.equal(translationModelIssue(settings, [llmProfile]), "profile_unavailable");

  settings.translation.model_selection.profile_id = "openai-1";
  assert.equal(translationModelIssue(settings, [llmProfile]), "model_required");

  settings.translation.model_selection.model = "gpt-5-mini";
  assert.equal(translationModelIssue(settings, [llmProfile]), null);
});

test("treats ready legacy target bindings and local PLaMo as ready", () => {
  const llmProfile = makeProfile();
  const settings = makeSettings();
  settings.translation.speaker_targets[0].profile_id = "openai-1";
  settings.translation.speaker_targets[0].model = "gpt-5-mini";
  settings.translation.microphone_targets[0].profile_id = "openai-1";
  settings.translation.microphone_targets[0].model = "gpt-5-mini";
  assert.equal(translationModelIssue(settings, [llmProfile]), null);

  const localSettings = makeSettings();
  localSettings.model_defaults.translation = {
    backend: "plamo_local",
    profile_id: null,
    model: "plamo-3-translate-0.8b-q4_k_m",
  };
  assert.equal(translationModelIssue(localSettings, []), null);
});

test("identifies which error codes should offer a jump to model assignments", () => {
  assert.equal(errorRequiresTranslationModelSettings("settings.translation_default_model_missing"), true);
  assert.equal(errorRequiresTranslationModelSettings("settings.translation_profile_missing"), true);
  assert.equal(errorRequiresTranslationModelSettings("settings.translation_model_required"), true);
  assert.equal(errorRequiresTranslationModelSettings("settings.translation_profile_unavailable"), true);
  assert.equal(errorRequiresTranslationModelSettings("settings.translation_language_unsupported"), false);
  assert.equal(errorRequiresTranslationModelSettings("settings.invalid"), false);
  assert.equal(errorRequiresTranslationModelSettings(null), false);
});

test("detects when live translation covers only single-target automatic mode", () => {
  const settings = makeSettings();
  assert.equal(liveTranslationOnlyNeedsModelForManualOrExtraTargets(settings), false);

  settings.asr.backend = "qwen_live_translate";
  assert.equal(liveTranslationOnlyNeedsModelForManualOrExtraTargets(settings), true);

  settings.translation.speaker_targets.push({
    ...settings.translation.speaker_targets[0],
    target_language: "fr",
  });
  assert.equal(liveTranslationOnlyNeedsModelForManualOrExtraTargets(settings), false);
});
