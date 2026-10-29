import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_FEATURE_SETTINGS, resolveSettingsCategory, visibleSettingsCategories } from "../src/settings/feature-availability.ts";

test("all modules retain the existing settings categories by default", () => {
  assert.deepEqual(visibleSettingsCategories(DEFAULT_FEATURE_SETTINGS), [
    "system", "audio", "recognition", "translation", "vr_overlay", "vision", "glossary", "api", "learning", "connections", "debug",
  ]);
});

test("disabled modules disappear while core settings remain available", () => {
  const features = Object.fromEntries(Object.keys(DEFAULT_FEATURE_SETTINGS).map((key) => [key, false])) as unknown as typeof DEFAULT_FEATURE_SETTINGS;
  assert.deepEqual(visibleSettingsCategories(features), ["system", "audio", "recognition", "translation", "api", "connections", "debug"]);
  assert.equal(resolveSettingsCategory("ocr", features), "system");
  assert.equal(resolveSettingsCategory("translation", features), "translation");
});

test("vision is independently visible and legacy OCR requests resolve to vision", () => {
  const features = { ...DEFAULT_FEATURE_SETTINGS, ocr: false, visual_question_answering: true, vr_overlay: false };
  assert.equal(visibleSettingsCategories(features).includes("vision"), true);
  assert.equal(resolveSettingsCategory("ocr", features), "vision");
  assert.equal(resolveSettingsCategory("vision", features), "vision");
  assert.equal(DEFAULT_FEATURE_SETTINGS.visual_question_answering, false);
  assert.equal(resolveSettingsCategory("ocr", DEFAULT_FEATURE_SETTINGS), "vision");
});

test("OSC connections remain available independently of optional features", () => {
  const features = { ...DEFAULT_FEATURE_SETTINGS, anki: false, vrcx: false, external_api: false };
  assert.equal(visibleSettingsCategories(features).includes("connections"), true);
  assert.equal(DEFAULT_FEATURE_SETTINGS.microphone_speaker_filter, false);
});

test("desktop OCR settings remain available when VR is disabled", () => {
  const features = { ...DEFAULT_FEATURE_SETTINGS, vr_overlay: false };
  assert.equal(visibleSettingsCategories(features).includes("vision"), true);
  assert.equal(resolveSettingsCategory("vr_overlay", features), "system");
});
