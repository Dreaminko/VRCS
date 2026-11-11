import assert from "node:assert/strict";
import test from "node:test";

import { captureUnavailableReason, DEFAULT_FEATURE_SETTINGS, resolveSettingsCategory, visibleSettingsCategories } from "../src/settings/feature-availability.ts";
import type { CoreCapabilities } from "../src/core-client/capabilities";
import type { Settings } from "../src/settings/types";

test("all modules retain the existing settings categories by default", () => {
  assert.deepEqual(visibleSettingsCategories(DEFAULT_FEATURE_SETTINGS), [
    "system", "audio", "recognition", "translation", "vr_overlay", "vision", "glossary", "api", "learning", "connections", "about",
  ]);
});

test("disabled modules disappear while core settings remain available", () => {
  const features = Object.fromEntries(Object.keys(DEFAULT_FEATURE_SETTINGS).map((key) => [key, false])) as unknown as typeof DEFAULT_FEATURE_SETTINGS;
  assert.deepEqual(visibleSettingsCategories(features), ["system", "audio", "recognition", "translation", "api", "connections", "about"]);
  assert.equal(resolveSettingsCategory("ocr", features), "system");
  assert.equal(resolveSettingsCategory("translation", features), "translation");
});

test("capture keeps its audio navigation alias without a separate experimental category", () => {
  for (const features of [DEFAULT_FEATURE_SETTINGS, { ...DEFAULT_FEATURE_SETTINGS, vr_overlay: false, learning: false }]) {
    assert.equal(visibleSettingsCategories(features).at(-1), "about");
    assert.equal(resolveSettingsCategory("about", features), "about");
    assert.equal(resolveSettingsCategory("audio", features), "audio");
  }
});

test("legacy debug navigation opens system settings without a separate tab", () => {
  assert.equal(visibleSettingsCategories(DEFAULT_FEATURE_SETTINGS).includes("debug"), false);
  assert.equal(resolveSettingsCategory("debug", DEFAULT_FEATURE_SETTINGS), "system");
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

test("a disabled network master leaves ordinary capture available with saved network consumers", () => {
  const available = { supported: true, available: true, reason_code: null };
  const network = { supported: true, available: false, reason_code: "platform.network_helper_missing" };
  const capabilities = {
    audio: { system_output: available, microphone: available, player_audio: network },
    chatbox: { network_receive: network },
  } as CoreCapabilities;
  for (const mode of ["system", "disabled", "vrchat_players"] as const) {
    const settings = {
      audio: { output: { mode, player_audio: { enabled: true } }, microphone: { mode: "default" } },
      vrchat_chatbox: { enabled: true },
      asr: { backend: "cloud" },
    } as Settings;
    assert.equal(captureUnavailableReason(settings, capabilities), "platform.network_helper_missing");
    settings.audio.output.player_audio!.enabled = false;
    assert.equal(captureUnavailableReason(settings, capabilities), null);
  }
});
