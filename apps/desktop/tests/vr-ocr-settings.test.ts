import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_OCR_SETTINGS, DEFAULT_VR_OVERLAY_SETTINGS, isVrOcrBackendReady, patchOcr } from "../src/settings/vr-overlay-settings.ts";
import type { Settings } from "../src/settings/types.ts";

test("OCR defaults use original-position display while allowing independent wrist and backend choices", () => {
  const settings = { vr_overlay: DEFAULT_VR_OVERLAY_SETTINGS, ocr: DEFAULT_OCR_SETTINGS } as Settings;
  assert.equal(settings.ocr.display_mode, "stereo");
  assert.equal(settings.ocr.desktop_enabled, false);
  assert.equal(settings.ocr.shortcut, "Ctrl+Alt+O");
  const patched = patchOcr(settings, { display_mode: "wrist", backend: "local", desktop_enabled: true });
  assert.equal(patched.ocr.display_mode, "wrist");
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

test("OCR acceleration is opt-in and does not change the recognition backend", () => {
  const settings = { ocr: DEFAULT_OCR_SETTINGS } as Settings;
  assert.equal(settings.ocr.device, "cpu");
  const patched = patchOcr(settings, { device: "directml" });
  assert.equal(patched.ocr.device, "directml");
  assert.equal(patched.ocr.backend, "cloud");
  assert.equal(settings.ocr.device, "cpu");
});

test("cloud OCR readiness requires a credential regardless of local model state", () => {
  assert.equal(isVrOcrBackendReady("cloud", true, "missing"), true);
  assert.equal(isVrOcrBackendReady("cloud", false, "ready"), false);
});

test("OCR wrist defaults have their own readable appearance and position", () => {
  assert.equal(DEFAULT_OCR_SETTINGS.wrist?.font_size_px, 32);
  assert.equal(DEFAULT_OCR_SETTINGS.wrist?.width_m, 0.32);
  assert.equal(DEFAULT_OCR_SETTINGS.wrist?.hand, "left");
  const settings = { vr_overlay: DEFAULT_VR_OVERLAY_SETTINGS, ocr: DEFAULT_OCR_SETTINGS } as Settings;
  const patched = patchOcr(settings, { wrist: { ...DEFAULT_OCR_SETTINGS.wrist!, width_m: 0.48 } });
  assert.equal(patched.ocr.wrist?.width_m, 0.48);
  assert.equal(settings.ocr.wrist?.width_m, 0.32);
  assert.equal(patched.vr_overlay.wrist.width_m, 0.32);
});
