import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_FEATURE_SETTINGS, resolveSettingsCategory, visibleSettingsCategories } from "../src/settings/feature-availability.ts";

test("all modules retain the existing settings categories by default", () => {
  assert.deepEqual(visibleSettingsCategories(DEFAULT_FEATURE_SETTINGS), [
    "system", "audio", "recognition", "translation", "glossary", "api", "learning", "connections", "ocr", "vr_overlay", "debug",
  ]);
});

test("disabled modules disappear while core settings remain available", () => {
  const features = Object.fromEntries(Object.keys(DEFAULT_FEATURE_SETTINGS).map((key) => [key, false])) as unknown as typeof DEFAULT_FEATURE_SETTINGS;
  assert.deepEqual(visibleSettingsCategories(features), ["system", "audio", "recognition", "translation", "api", "debug"]);
  assert.equal(resolveSettingsCategory("ocr", features), "system");
  assert.equal(resolveSettingsCategory("translation", features), "translation");
});

test("connections remain visible when any integration is available", () => {
  const features = { ...DEFAULT_FEATURE_SETTINGS, anki: false, osc_chatbox: false, vrcx: false, external_api: false };
  assert.equal(visibleSettingsCategories(features).includes("connections"), false);
  for (const key of ["anki", "osc_chatbox", "vrcx", "external_api"] as const) {
    assert.equal(visibleSettingsCategories({ ...features, [key]: true }).includes("connections"), true);
  }
});

test("desktop OCR settings remain available when VR is disabled", () => {
  const features = { ...DEFAULT_FEATURE_SETTINGS, vr_overlay: false };
  assert.equal(visibleSettingsCategories(features).includes("ocr"), true);
  assert.equal(resolveSettingsCategory("vr_overlay", features), "system");
});
