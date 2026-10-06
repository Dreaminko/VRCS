import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_OCR_SETTINGS, DEFAULT_VR_OVERLAY_SETTINGS, isVrOcrBackendReady, patchOcr } from "../src/settings/vr-overlay-settings.ts";
import type { Settings } from "../src/settings/types.ts";

test("OCR defaults preserve wrist display while allowing independent stereo and backend choices", () => {
  const settings = { vr_overlay: DEFAULT_VR_OVERLAY_SETTINGS, ocr: DEFAULT_OCR_SETTINGS } as Settings;
  assert.equal(settings.ocr.display_mode, "wrist");
  assert.equal(settings.ocr.desktop_enabled, false);
  assert.equal(settings.ocr.shortcut, "Ctrl+Alt+O");
  const patched = patchOcr(settings, { display_mode: "stereo", backend: "local", desktop_enabled: true });
  assert.equal(patched.ocr.display_mode, "stereo");
  assert.equal(patched.ocr.backend, "local");
  assert.equal(patched.ocr.desktop_enabled, true);
  assert.equal(patched.ocr.enabled, false);
  assert.equal(patched.vr_overlay, settings.vr_overlay);
  assert.equal(settings.ocr.backend, "cloud");
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
