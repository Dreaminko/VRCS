import assert from "node:assert/strict";
import test from "node:test";
import type { TestContext } from "node:test";

import {
  learningPreferencesSnapshot,
  readLearningPreferences,
  subscribeLearningPreferences,
  updateLearningPreferences,
} from "../src/learning/preferences.ts";

test("follows the app language for new, missing, or invalid preferences", () => {
  for (const value of [null, "{", "{}", '{"explanationLanguage":"invalid locale"}']) {
    assert.equal(readLearningPreferences(value).explanationLanguage, "ui");
  }
});

test("preserves saved language choices, AI configuration, and legacy detail levels", () => {
  for (const explanationLanguage of ["zh-CN", "en-US", "ja-JP", "zh-Hant", "fr-FR", "ui"]) {
    const saved = { profileId: "profile-one", model: "model-one", explanationLanguage };
    assert.deepEqual(readLearningPreferences(JSON.stringify({
      ...saved,
      explanationLevel: "standard",
    })), { ...saved, explanationLevel: "intermediate" });
  }
});

function mockWindow(t: TestContext) {
  const previous = Object.getOwnPropertyDescriptor(globalThis, "window");
  let saved: string | null = null;
  const target = Object.assign(new EventTarget(), {
    localStorage: {
      getItem: () => saved,
      setItem: (_key: string, value: string) => { saved = value; },
    },
  });
  Object.defineProperty(globalThis, "window", { configurable: true, value: target });
  t.after(() => {
    if (previous) Object.defineProperty(globalThis, "window", previous);
    else Reflect.deleteProperty(globalThis, "window");
  });
  return target;
}

test("synchronizes consumers immediately and preserves follow mode when saving a model", (t) => {
  mockWindow(t);
  const first: string[] = [];
  const second: string[] = [];
  const readLanguage = () => readLearningPreferences(learningPreferencesSnapshot()).explanationLanguage;
  const stopFirst = subscribeLearningPreferences(() => first.push(readLanguage()));
  const stopSecond = subscribeLearningPreferences(() => second.push(readLanguage()));
  try {
    updateLearningPreferences((current) => ({ ...current, model: "model-one" }));
    assert.equal(readLanguage(), "ui");
    updateLearningPreferences((current) => ({ ...current, explanationLanguage: "zh-Hant" }));
    assert.deepEqual(first, ["ui", "zh-Hant"]);
    assert.deepEqual(second, first);
    assert.equal(readLearningPreferences(learningPreferencesSnapshot()).model, "model-one");

    stopFirst();
    updateLearningPreferences((current) => ({ ...current, explanationLanguage: "ui" }));
    assert.deepEqual(first, ["ui", "zh-Hant"]);
    assert.deepEqual(second, ["ui", "zh-Hant", "ui"]);
    assert.equal(readLanguage(), "ui");
  } finally {
    stopFirst();
    stopSecond();
  }
});

test("notifies consumers when another window changes or clears preferences", (t) => {
  const target = mockWindow(t);
  let changes = 0;
  const stop = subscribeLearningPreferences(() => { changes += 1; });
  try {
    for (const key of ["unrelated", "vrcs.learning.preferences.v1", null]) {
      target.dispatchEvent(Object.assign(new Event("storage"), { key }));
    }
    assert.equal(changes, 2);
  } finally {
    stop();
  }
});
