import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_FEATURE_SETTINGS } from "../src/settings/feature-availability.ts";
import {
  applyFeaturePreset,
  areFeatureSettingsEqual,
  FEATURE_KEYS,
  matchFeaturePreset,
} from "../src/settings/feature-presets.ts";
import type { FeatureSettings } from "../src/settings/types.ts";

test("translation_only keeps glossary enabled, disables optional modules, and preserves vr_overlay", () => {
  const withVrOn: FeatureSettings = { ...DEFAULT_FEATURE_SETTINGS, glossary: false, vr_overlay: true };
  const snapshot = { ...withVrOn };
  const appliedOn = applyFeaturePreset(withVrOn, "translation_only");

  assert.deepEqual(withVrOn, snapshot);
  assert.deepEqual(appliedOn, {
    glossary: true,
    learning: false,
    anki: false,
    vrcx: false,
    ocr: false,
    visual_question_answering: false,
    vr_overlay: true,
    external_api: false,
  });
  assert.equal(matchFeaturePreset(appliedOn), "translation_only");
  assert.deepEqual(applyFeaturePreset(appliedOn, "translation_only"), appliedOn);

  const withVrOff: FeatureSettings = { ...DEFAULT_FEATURE_SETTINGS, vr_overlay: false };
  const appliedOff = applyFeaturePreset(withVrOff, "translation_only");
  assert.equal(appliedOff.vr_overlay, false);
  assert.equal(appliedOff.glossary, true);
  assert.equal(matchFeaturePreset(appliedOff), "translation_only");
});

test("full preset enables every feature switch including vr_overlay", () => {
  const starting: FeatureSettings = {
    glossary: false,
    learning: false,
    anki: false,
    vrcx: false,
    ocr: false,
    visual_question_answering: false,
    vr_overlay: false,
    external_api: false,
  };
  const applied = applyFeaturePreset(starting, "full");

  for (const key of FEATURE_KEYS) {
    assert.equal(applied[key], true, `expected ${key} to be enabled`);
  }
  assert.equal(starting.vr_overlay, false);
  assert.equal(matchFeaturePreset(applied), "full");
  assert.deepEqual(applyFeaturePreset(applied, "full"), applied);
});

test("matchFeaturePreset returns null for default or custom combinations", () => {
  assert.equal(matchFeaturePreset(DEFAULT_FEATURE_SETTINGS), null);

  const translationOnly = applyFeaturePreset(DEFAULT_FEATURE_SETTINGS, "translation_only");
  assert.equal(matchFeaturePreset({ ...translationOnly, glossary: false }), null);
  assert.equal(matchFeaturePreset({ ...translationOnly, learning: true }), null);

  const full = applyFeaturePreset(DEFAULT_FEATURE_SETTINGS, "full");
  assert.equal(matchFeaturePreset({ ...full, vr_overlay: false }), null);
  assert.equal(matchFeaturePreset({ ...full, visual_question_answering: false }), null);
});

test("areFeatureSettingsEqual compares all feature keys accurately", () => {
  const a = applyFeaturePreset(DEFAULT_FEATURE_SETTINGS, "translation_only");
  const b = { ...a };
  assert.equal(areFeatureSettingsEqual(a, b), true);
  assert.equal(areFeatureSettingsEqual(a, { ...b, vr_overlay: !b.vr_overlay }), false);
  assert.equal(areFeatureSettingsEqual(a, { ...b, glossary: false }), false);
});
