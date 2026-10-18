import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_OCR_SETTINGS, DEFAULT_VR_OVERLAY_SETTINGS, isOcrReady, patchOcr } from "../src/settings/vr-overlay-settings.ts";
import type { Settings } from "../src/settings/types.ts";

test("OCR defaults use original-position display while allowing independent desktop and wrist choices", () => {
  const settings = { vr_overlay: DEFAULT_VR_OVERLAY_SETTINGS, ocr: DEFAULT_OCR_SETTINGS } as Settings;
  assert.equal(settings.ocr.display_mode, "stereo");
  assert.equal(settings.ocr.desktop_enabled, false);
  assert.equal(settings.ocr.shortcut, "Ctrl+Alt+O");
  const patched = patchOcr(settings, { display_mode: "wrist", desktop_enabled: true });
  assert.equal(patched.ocr.display_mode, "wrist");
  assert.equal(patched.ocr.desktop_enabled, true);
  assert.equal(patched.ocr.enabled, false);
  assert.equal(patched.vr_overlay, settings.vr_overlay);
  assert.equal(settings.ocr.desktop_enabled, false);
});

test("OCR master switch preserves mode preferences and recognition settings", () => {
  const settings = { ocr: { ...DEFAULT_OCR_SETTINGS, enabled: true, desktop_enabled: true } } as Settings;
  assert.equal(DEFAULT_OCR_SETTINGS.master_enabled, true);
  const disabled = patchOcr(settings, { master_enabled: false });
  assert.deepEqual(disabled.ocr, { ...settings.ocr, master_enabled: false });
  const enabled = patchOcr(disabled, { master_enabled: true });
  assert.deepEqual(enabled.ocr, settings.ocr);
  assert.equal(settings.ocr.master_enabled, true);
});

test("OCR requires prepared local models", () => {
  assert.equal(isOcrReady("ready"), true);
  for (const state of [undefined, "missing", "downloading", "error"] as const) {
    assert.equal(isOcrReady(state), false);
  }
});

test("OCR acceleration is opt-in", () => {
  const settings = { ocr: DEFAULT_OCR_SETTINGS } as Settings;
  assert.equal(settings.ocr.device, "cpu");
  const patched = patchOcr(settings, { device: "directml" });
  assert.equal(patched.ocr.device, "directml");
  assert.equal(settings.ocr.device, "cpu");
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
