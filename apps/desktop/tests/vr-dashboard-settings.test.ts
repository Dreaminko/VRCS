import assert from "node:assert/strict";
import test from "node:test";

import { applyVrDashboardAction } from "../src/vr-dashboard/settings-actions.ts";
import { DEFAULT_VR_OVERLAY_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import type { Settings } from "../src/settings/types.ts";

const settings = {
  schema_version: 27,
  vr_overlay: {
    ...DEFAULT_VR_OVERLAY_SETTINGS,
    headset: { ...DEFAULT_VR_OVERLAY_SETTINGS.headset },
    wrist: { ...DEFAULT_VR_OVERLAY_SETTINGS.wrist },
    ocr: { ...DEFAULT_VR_OVERLAY_SETTINGS.ocr },
  },
  untouched: { value: 42 },
} as unknown as Settings;

test("dashboard master action changes only the VR Overlay master switch", () => {
  const next = applyVrDashboardAction(settings, "toggle_master");

  assert.equal(next.vr_overlay.enabled, true);
  assert.equal(next.vr_overlay.headset, settings.vr_overlay.headset);
  assert.equal((next as unknown as { untouched: object }).untouched,
    (settings as unknown as { untouched: object }).untouched);
});

test("dashboard content actions cycle the shared translation display", () => {
  for (const action of ["cycle_headset_content", "cycle_wrist_content"] as const) {
    const preferredOnly = applyVrDashboardAction(settings, action);
    const allLanguages = applyVrDashboardAction(preferredOnly, action);

    assert.equal(allLanguages.vr_overlay.translation_display, "all_languages");
    assert.equal(preferredOnly.vr_overlay.translation_display, "preferred_only");
    assert.equal(preferredOnly.vr_overlay.headset, settings.vr_overlay.headset);
    assert.equal(preferredOnly.vr_overlay.wrist, settings.vr_overlay.wrist);
  }
});

test("dashboard numeric steps clamp to desktop control limits", () => {
  const narrow = {
    ...settings,
    vr_overlay: {
      ...settings.vr_overlay,
      headset: { ...settings.vr_overlay.headset, width_m: 0.2, opacity: 0.1 },
    },
  };

  const width = applyVrDashboardAction(narrow, "headset_width_down");
  const opacity = applyVrDashboardAction(width, "headset_opacity_down");

  assert.equal(width.vr_overlay.headset.width_m, 0.2);
  assert.equal(opacity.vr_overlay.headset.opacity, 0.1);
});

test("dashboard wrist hand action cycles without changing headset settings", () => {
  const right = applyVrDashboardAction(settings, "cycle_wrist_hand");
  const dominant = applyVrDashboardAction(right, "cycle_wrist_hand");
  const left = applyVrDashboardAction(dominant, "cycle_wrist_hand");

  assert.equal(right.vr_overlay.wrist.hand, "right");
  assert.equal(dominant.vr_overlay.wrist.hand, "dominant");
  assert.equal(left.vr_overlay.wrist.hand, "left");
  assert.equal(left.vr_overlay.headset, settings.vr_overlay.headset);
});
