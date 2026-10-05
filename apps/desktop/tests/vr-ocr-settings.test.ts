import assert from "node:assert/strict";
import test from "node:test";

import { isVrOcrBackendReady } from "../src/settings/vr-overlay-settings.ts";

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
