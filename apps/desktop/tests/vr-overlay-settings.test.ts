import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_VR_OVERLAY_HEADSET_SETTINGS,
  DEFAULT_VR_OVERLAY_SETTINGS,
  DEFAULT_VR_OVERLAY_WRIST_SETTINGS,
  patchVrOverlay,
  patchVrOverlayHeadset,
  patchVrOverlayWrist,
  resetVrOverlayHeadset,
  resetVrOverlayWrist,
  setVrOverlayHeadsetDisplaySeconds,
} from "../src/settings/vr-overlay-settings.ts";
import type { Settings } from "../src/types.ts";
import {
  getVrOverlayStatus,
  hideVrOverlaySample,
  listenVrOverlayStatus,
  retryVrOverlay,
  showVrOverlaySample,
  UNSUPPORTED_VR_OVERLAY_STATUS,
  VR_OVERLAY_STATUS_EVENT,
} from "../src/vr-overlay-native.ts";

const settings = {
  schema_version: 26,
  vr_overlay: DEFAULT_VR_OVERLAY_SETTINGS,
} as unknown as Settings;

test("VR Overlay defaults match the complete schema v23 contract", () => {
  assert.deepEqual(Object.keys(DEFAULT_VR_OVERLAY_SETTINGS).sort(), ["enabled", "headset", "wrist"]);
  assert.deepEqual(Object.keys(DEFAULT_VR_OVERLAY_HEADSET_SETTINGS).sort(), [
    "background_opacity",
    "content_mode",
    "display_seconds",
    "distance_m",
    "enabled",
    "fade_seconds",
    "font_size_px",
    "include_chatbox",
    "include_microphone",
    "include_speaker",
    "offset_x_m",
    "offset_y_m",
    "opacity",
    "pitch_deg",
    "roll_deg",
    "show_partials",
    "show_translation_partials",
    "vr_drag_edit_enabled",
    "width_m",
    "yaw_deg",
  ]);
  assert.equal(DEFAULT_VR_OVERLAY_HEADSET_SETTINGS.show_partials, false);
  assert.equal(DEFAULT_VR_OVERLAY_WRIST_SETTINGS.show_partials, false);
  assert.deepEqual(Object.keys(DEFAULT_VR_OVERLAY_WRIST_SETTINGS).sort(), [
    "background_opacity",
    "content_mode",
    "dominant_hand",
    "enabled",
    "font_size_px",
    "hand",
    "idle_hide_seconds",
    "include_chatbox",
    "include_microphone",
    "include_speaker",
    "max_entries",
    "offset_x_m",
    "offset_y_m",
    "offset_z_m",
    "opacity",
    "pitch_deg",
    "roll_deg",
    "show_partials",
    "show_translation_partials",
    "width_m",
    "yaw_deg",
  ]);
});

test("VR Overlay patches are immutable and scoped to the requested branch", () => {
  const enabled = patchVrOverlay(settings, { enabled: true });
  const headset = patchVrOverlayHeadset(enabled, { distance_m: 2.4 });
  const wrist = patchVrOverlayWrist(headset, { hand: "right", max_entries: 8 });

  assert.equal(settings.vr_overlay.enabled, false);
  assert.equal(enabled.vr_overlay.enabled, true);
  assert.equal(headset.vr_overlay.headset.distance_m, 2.4);
  assert.equal(headset.vr_overlay.wrist, settings.vr_overlay.wrist);
  assert.equal(wrist.vr_overlay.wrist.hand, "right");
  assert.equal(wrist.vr_overlay.wrist.max_entries, 8);
  assert.equal(wrist.vr_overlay.headset, headset.vr_overlay.headset);
});

test("headset display duration clamps fade duration in the same immutable patch", () => {
  const changed = patchVrOverlayHeadset(settings, { display_seconds: 6, fade_seconds: 4 });
  const shortened = setVrOverlayHeadsetDisplaySeconds(changed, 2.5);
  const lengthened = setVrOverlayHeadsetDisplaySeconds(shortened, 8);

  assert.equal(changed.vr_overlay.headset.display_seconds, 6);
  assert.equal(changed.vr_overlay.headset.fade_seconds, 4);
  assert.equal(shortened.vr_overlay.headset.display_seconds, 2.5);
  assert.equal(shortened.vr_overlay.headset.fade_seconds, 2.5);
  assert.equal(lengthened.vr_overlay.headset.display_seconds, 8);
  assert.equal(lengthened.vr_overlay.headset.fade_seconds, 2.5);
});

test("headset and wrist reset independently without changing the master switch", () => {
  const changed = patchVrOverlayWrist(
    patchVrOverlayHeadset(patchVrOverlay(settings, { enabled: true }), {
      opacity: 0.35,
      width_m: 2.5,
    }),
    { opacity: 0.4, max_entries: 9 },
  );

  const resetHeadset = resetVrOverlayHeadset(changed);
  assert.equal(resetHeadset.vr_overlay.enabled, true);
  assert.deepEqual(resetHeadset.vr_overlay.headset, DEFAULT_VR_OVERLAY_HEADSET_SETTINGS);
  assert.equal(resetHeadset.vr_overlay.wrist, changed.vr_overlay.wrist);

  const resetWrist = resetVrOverlayWrist(changed);
  assert.equal(resetWrist.vr_overlay.enabled, true);
  assert.deepEqual(resetWrist.vr_overlay.wrist, DEFAULT_VR_OVERLAY_WRIST_SETTINGS);
  assert.equal(resetWrist.vr_overlay.headset, changed.vr_overlay.headset);
});

test("native VR Overlay wrapper is safe outside Tauri", async () => {
  assert.equal(VR_OVERLAY_STATUS_EVENT, "vr-overlay-status-changed");
  assert.deepEqual(await getVrOverlayStatus(), UNSUPPORTED_VR_OVERLAY_STATUS);
  await retryVrOverlay();
  await showVrOverlaySample("headset");
  await hideVrOverlaySample("wrist");
  const unlisten = await listenVrOverlayStatus(() => assert.fail("browser fallback must not emit"));
  unlisten();
});
