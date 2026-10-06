import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_VR_OVERLAY_SETTINGS, isVrOcrBackendReady, patchVrOverlay } from "../src/settings/vr-overlay-settings.ts";
import type { Settings } from "../src/settings/types.ts";

test("OCR defaults preserve wrist display while allowing independent stereo and backend choices", () => {
  const defaults = DEFAULT_VR_OVERLAY_SETTINGS;
  assert.equal(defaults.ocr.display_mode, "wrist");
  const settings = { vr_overlay: defaults } as Settings;
  const patched = patchVrOverlay(settings, {
    ocr: { ...defaults.ocr, display_mode: "stereo", backend: "local" },
  });
  assert.equal(patched.vr_overlay.ocr.display_mode, "stereo");
  assert.equal(patched.vr_overlay.ocr.backend, "local");
  assert.equal(settings.vr_overlay.ocr.backend, "cloud");
});

test("local OCR requires prepared models and does not require a cloud credential", () => {
  assert.equal(isVrOcrBackendReady("local", false, "ready"), true);
  for (const state of [undefined, "missing", "downloading", "error"] as const) {
    assert.equal(isVrOcrBackendReady("local", true, state), false);
  }
});

test("cloud OCR readiness requires a credential regardless of local model state", () => {
  assert.equal(isVrOcrBackendReady("cloud", true, "missing"), true);
  assert.equal(isVrOcrBackendReady("cloud", false, "ready"), false);
});
