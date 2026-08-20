import assert from "node:assert/strict";
import test from "node:test";

import type { SelectionTarget } from "../src/app/app-types.ts";
import type { LearningPreferences } from "../src/learning/hooks/useLearningWorkspace.ts";
import {
  selectionAiConfigured,
  selectionQueryInput,
} from "../src/selection/selection-ai.ts";

const preferences: LearningPreferences = {
  profileId: "profile-one",
  model: " model-one ",
  explanationLanguage: "zh-CN",
  explanationLevel: "intermediate",
};

const target = {
  selectedText: "気になる",
  context: "結果が気になる。",
  origin: {
    id: 7,
    language: "ja",
    source: "speaker",
    createdAt: "2026-01-01T00:00:00Z",
    translation: "我很在意结果。",
  },
  anchor: { top: 10, bottom: 30, centerX: 40 },
  range: {} as Range,
} satisfies SelectionTarget;

test("requires both an AI profile and a model", () => {
  assert.equal(selectionAiConfigured(preferences), true);
  assert.equal(selectionAiConfigured({ ...preferences, profileId: "" }), false);
  assert.equal(selectionAiConfigured({ ...preferences, model: "  " }), false);
});

test("builds a selection query from the shared selection target", () => {
  assert.deepEqual(selectionQueryInput(target, "  这是什么意思？  ", preferences), {
    selected_text: "気になる",
    source_text: "結果が気になる。",
    source_translation: "我很在意结果。",
    source_language: "ja",
    question: "这是什么意思？",
    profile_id: "profile-one",
    model: "model-one",
    explanation_language: "zh-CN",
    level: "intermediate",
  });
});
