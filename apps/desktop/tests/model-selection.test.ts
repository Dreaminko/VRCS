import assert from "node:assert/strict";
import test from "node:test";
import { applyModelDefaults, learningModelSelection } from "../src/settings/model-selection.ts";
import type { Settings } from "../src/settings/types.ts";

const binding = (model: string, use_default_model = true) => ({ profile_id: "profile", model, use_default_model });
function settings(): Settings {
  return {
    model_defaults: {
      text: binding("text"),
      translation: binding("translation"),
      vision: binding("image"),
    },
    translation: {
      model_selection: binding("old"),
      speaker_targets: [{ ...binding("old"), target_language: "en" }],
      microphone_targets: [{ ...binding("custom", false), target_language: "ja" }],
    },
    ocr: { model_selection: binding("old"), targets: [{ ...binding("old"), target_language: "zh-Hans" }] },
    conversation_titles: binding("old"), vision: binding("old"),
    learning_ai: { ...binding("old"), thinking_enabled: false },
  } as unknown as Settings;
}

test("resolves defaults through task selections and shares them with every target", () => {
  const original = settings();
  const result = applyModelDefaults(original);
  assert.equal(result.translation.speaker_targets[0].model, "translation");
  assert.equal(result.translation.speaker_targets[0].use_default_model, true);
  assert.equal(result.translation.microphone_targets[0].model, "translation");
  assert.equal(result.translation.microphone_targets[0].use_default_model, true);
  assert.equal(result.ocr.targets[0].model, "translation");
  assert.equal(result.conversation_titles.model, "text");
  assert.equal(result.vision.model, "image");
  assert.equal(result.learning_ai.model, "text");
  assert.equal(original.vision.model, "old");
  result.model_defaults = { ...result.model_defaults, text: binding("new") };
  const updated = applyModelDefaults(result);
  assert.equal(updated.learning_ai.model, "new");
  assert.equal(updated.ocr.targets[0].model, "translation");
  assert.equal(updated.translation.speaker_targets[0].model, "translation");
  updated.model_defaults = { ...updated.model_defaults, translation: binding("translation-two") };
  const translated = applyModelDefaults(updated);
  assert.equal(translated.ocr.targets[0].model, "translation-two");
  assert.equal(translated.translation.speaker_targets[0].model, "translation-two");
  assert.equal(translated.translation.microphone_targets[0].model, "translation-two");
});

test("missing legacy inheritance flags preserve saved choices and learning uses Core", () => {
  const original = settings();
  delete original.vision.use_default_model;
  original.learning_ai = { ...binding("learning", false), thinking_enabled: true };
  assert.equal(applyModelDefaults(original).vision.model, "old");
  assert.equal(learningModelSelection(original).model, "learning");
  assert.equal(learningModelSelection(original).thinking_enabled, true);
});

test("inherited local translation disables thinking", () => {
  const original = settings();
  original.translation.model_selection = {
    backend: "plamo_local",
    profile_id: null,
    model: "plamo-3-translate-0.8b-q4_k_m",
    use_default_model: false,
  };
  original.translation.speaker_targets[0] = {
    ...original.translation.speaker_targets[0],
    backend: "api",
    profile_id: "profile",
    model: "old",
    use_default_model: true,
    thinking_enabled: true,
    reasoning_control: { type: "effort", value: "high" },
  };

  const resolved = applyModelDefaults(original).translation.speaker_targets[0];

  assert.equal(resolved.backend, "plamo_local");
  assert.equal(resolved.thinking_enabled, false);
  assert.equal(resolved.reasoning_control, null);
});
