import assert from "node:assert/strict";
import test from "node:test";

import { applyVrDashboardAction, dashboardLanguageChoices } from "../src/vr-dashboard/settings-actions.ts";
import { DEFAULT_OCR_SETTINGS, DEFAULT_VR_OVERLAY_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import type { Settings } from "../src/settings/types.ts";
import type { ApiProfileView } from "../src/providers/types.ts";
import { VR_OVERLAY_POSITION_RANGES, type VrOverlayPositionField } from "../src/settings/vr-overlay-settings.ts";

const settings = {
  schema_version: 29,
  asr: { backend: "qwen_local_managed", language: "auto", managed_qwen: { package_id: "qwen3-asr-0.6b-q8_0", device: "auto" } },
  translation: {
    mode: "automatic",
    speaker_targets: [
      { target_language: "en", profile_id: "translator", model: "existing-model", thinking_enabled: true },
      { target_language: "ja", profile_id: "translator", model: "other-model", thinking_enabled: false },
    ],
    microphone_targets: [
      { target_language: "zh-Hans", profile_id: "translator", model: "mic-model", thinking_enabled: false },
    ],
    prompt: { system_prompt: "Keep this prompt" },
  },
  osc: {
    enabled: false, port: 9001, mute_sync_enabled: true, mute_status_toast_enabled: true,
    preserve_original_text: true, translation_strategy: "preferred_only",
  },
  language_presets: [],
  vr_overlay: {
    ...DEFAULT_VR_OVERLAY_SETTINGS,
    headset: { ...DEFAULT_VR_OVERLAY_SETTINGS.headset },
    wrist: { ...DEFAULT_VR_OVERLAY_SETTINGS.wrist },
  },
  ocr: { ...DEFAULT_OCR_SETTINGS },
  untouched: { value: 42 },
} as unknown as Settings;

const profiles = [{
  id: "translator", enabled_capabilities: ["text_translation"],
  capabilities: { supports_translation: true, supported_languages: ["en", "ja", "zh-Hans", "fr"] },
}] as unknown as ApiProfileView[];

test("dashboard language selection preserves the recognition engine and rejects invalid values", () => {
  const next = applyVrDashboardAction(settings, { set_recognition_language: "ja" }, profiles);
  assert.equal(next.asr.language, "ja");
  assert.equal(next.asr.managed_qwen, settings.asr.managed_qwen);
  assert.equal(next.translation, settings.translation);
  assert.equal(applyVrDashboardAction(settings, { set_recognition_language: "invalid" }, profiles), settings);
  const live = { ...settings, asr: { ...settings.asr, backend: "gemini_live_translate" } };
  assert.equal(applyVrDashboardAction(live, { set_recognition_language: "ja" }, profiles), live);
});

test("dashboard translation selection changes only the first route and respects provider languages", () => {
  const next = applyVrDashboardAction(settings, { set_target_language: { group: "speaker", index: 0, value: "fr" } }, profiles);
  assert.deepEqual(next.translation.speaker_targets[0], {
    ...settings.translation.speaker_targets[0], target_language: "fr",
  });
  assert.equal(next.translation.speaker_targets[1], settings.translation.speaker_targets[1]);
  assert.equal(next.translation.microphone_targets, settings.translation.microphone_targets);
  assert.equal(next.translation.prompt, settings.translation.prompt);
  assert.equal(applyVrDashboardAction(settings, { set_target_language: { group: "speaker", index: 0, value: "ja" } }, profiles), settings);
  assert.equal(applyVrDashboardAction(settings, { set_target_language: { group: "speaker", index: 0, value: "ko" } }, profiles), settings);
  assert.equal(applyVrDashboardAction(settings, { set_target_language: { group: "speaker", index: 0, value: "fr" } }).translation.speaker_targets[0].target_language, "fr");
});

test("all routes use desktop language filtering, including secondary live routes", () => {
  const live = { ...settings, asr: { ...settings.asr, backend: "gemini_live_translate" } };
  const choices = dashboardLanguageChoices(live, profiles);
  assert.ok(choices.speaker_targets[0].includes("ko"));
  assert.ok(!choices.speaker_targets[1].includes("ko"));
  assert.ok(choices.speaker_targets[1].includes("ja"));
  assert.ok(!choices.speaker_targets[1].includes("en"));
  const next = applyVrDashboardAction(settings, { set_target_language: { group: "speaker", index: 1, value: "fr" } }, profiles);
  assert.equal(next.translation.speaker_targets[1].target_language, "fr");
  assert.equal(next.translation.speaker_targets[1].model, "other-model");
  assert.equal(next.translation.speaker_targets[0], settings.translation.speaker_targets[0]);
  const emptyLanguages = [{ ...profiles[0], capabilities: { ...profiles[0].capabilities, supported_languages: [] } }];
  assert.deepEqual(dashboardLanguageChoices(settings, emptyLanguages).speaker_targets[0], []);
});

test("VR routes have desktop add, move and delete limits and preserve route configuration", () => {
  const added = applyVrDashboardAction(settings, { add_target: "speaker" }, profiles);
  assert.deepEqual(added.translation.speaker_targets[2], { target_language: "zh-Hans", profile_id: "translator", model: "gpt-5-mini", thinking_enabled: false });
  assert.equal(applyVrDashboardAction(added, { add_target: "speaker" }, profiles), added);
  assert.equal(applyVrDashboardAction(settings, { add_target: "speaker" }), settings);
  const moved = applyVrDashboardAction(added, { move_target: { group: "speaker", index: 2, offset: -1 } }, profiles);
  assert.equal(moved.translation.speaker_targets[1], added.translation.speaker_targets[2]);
  assert.equal(moved.translation.speaker_targets[2], added.translation.speaker_targets[1]);
  assert.equal(applyVrDashboardAction(settings, { move_target: { group: "speaker", index: 0, offset: -1 } }, profiles), settings);
  assert.equal(applyVrDashboardAction(settings, { delete_target: { group: "microphone", index: 0 } }, profiles), settings);
  assert.equal(applyVrDashboardAction(settings, { delete_target: { group: "speaker", index: 8 } }, profiles), settings);
  const removed = applyVrDashboardAction(moved, { delete_target: { group: "speaker", index: 1 } }, profiles);
  assert.equal(removed.translation.speaker_targets.length, 2);
  assert.equal(removed.translation.microphone_targets, settings.translation.microphone_targets);
});

test("VR language presets restore complete desktop language and OSC configurations", () => {
  const saved = applyVrDashboardAction(settings, { save_language_preset: "Preset 1" }, profiles);
  assert.equal(saved.language_presets.length, 1);
  assert.deepEqual(saved.language_presets[0].speaker_targets, settings.translation.speaker_targets);
  assert.notEqual(saved.language_presets[0].speaker_targets[0], settings.translation.speaker_targets[0]);
  const changed = { ...saved, asr: { ...saved.asr, language: "fr" as const }, translation: { ...saved.translation, speaker_targets: [] }, osc: { ...saved.osc, translation_strategy: "all_languages" as const } };
  const restored = applyVrDashboardAction(changed, { apply_language_preset: saved.language_presets[0].id }, profiles);
  assert.equal(restored.asr.language, settings.asr.language);
  assert.deepEqual(restored.translation.speaker_targets, settings.translation.speaker_targets);
  assert.equal(restored.osc.translation_strategy, settings.osc.translation_strategy);
  assert.equal(restored.translation.prompt, settings.translation.prompt);
  assert.equal(applyVrDashboardAction(saved, { apply_language_preset: "missing" }, profiles), saved);
  assert.equal(applyVrDashboardAction(saved, { delete_language_preset: saved.language_presets[0].id }, profiles).language_presets.length, 0);
  const full = { ...saved, language_presets: Array.from({ length: 5 }, () => saved.language_presets[0]) };
  assert.equal(applyVrDashboardAction(full, { save_language_preset: "Preset 6" }, profiles), full);
});

test("dashboard microphone language and translation mode preserve unrelated settings", () => {
  const next = applyVrDashboardAction(settings, { set_target_language: { group: "microphone", index: 0, value: "fr" } }, profiles);
  assert.equal(next.translation.microphone_targets[0].target_language, "fr");
  assert.equal(next.translation.microphone_targets[0].model, "mic-model");
  assert.equal(next.translation.speaker_targets, settings.translation.speaker_targets);
  const manual = applyVrDashboardAction(next, { set_translation_mode: "manual" }, profiles);
  assert.equal(manual.translation.mode, "manual");
  assert.equal(manual.translation.microphone_targets, next.translation.microphone_targets);
  assert.equal(applyVrDashboardAction(settings, { set_translation_mode: "invalid" }, profiles), settings);
  const empty = { ...settings, translation: { ...settings.translation, speaker_targets: [] } };
  assert.equal(applyVrDashboardAction(empty, { set_target_language: { group: "speaker", index: 0, value: "fr" } }, profiles), empty);
});

test("live translation language selection uses the live service's supported targets", () => {
  const live = { ...settings, asr: { ...settings.asr, backend: "gemini_live_translate" } };
  assert.equal(applyVrDashboardAction(live, { set_target_language: { group: "microphone", index: 0, value: "nl" } }), live);
  assert.equal(applyVrDashboardAction(live, { set_target_language: { group: "microphone", index: 0, value: "yue-Hant" } }), live);
  assert.equal(applyVrDashboardAction(live, { set_target_language: { group: "microphone", index: 0, value: "ko" } }).translation.microphone_targets[0].target_language, "ko");
});

test("dashboard OSC settings preserve the endpoint and cycle all translation strategies", () => {
  const fields = [
    ["toggle_osc", "enabled"], ["toggle_osc_original", "preserve_original_text"],
    ["toggle_osc_mute_sync", "mute_sync_enabled"], ["toggle_osc_mute_toast", "mute_status_toast_enabled"],
  ] as const;
  for (const [action, key] of fields) {
    const next = applyVrDashboardAction(settings, action);
    assert.deepEqual(next.osc, { ...settings.osc, [key]: !settings.osc[key] });
    assert.equal(next.translation, settings.translation);
    assert.equal(next.vr_overlay, settings.vr_overlay);
  }
  const roundRobin = applyVrDashboardAction(settings, "cycle_osc_strategy");
  const all = applyVrDashboardAction(roundRobin, "cycle_osc_strategy");
  const preferred = applyVrDashboardAction(all, "cycle_osc_strategy");
  assert.equal(roundRobin.osc.translation_strategy, "round_robin");
  assert.equal(all.osc.translation_strategy, "all_languages");
  assert.deepEqual(preferred.osc, settings.osc);
});

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
      headset: { ...settings.vr_overlay.headset, width_m: 0.25, opacity: 0.1 },
    },
  };

  const width = applyVrDashboardAction(narrow, "headset_width_down");
  const opacity = applyVrDashboardAction(width, "headset_opacity_down");

  assert.equal(width.vr_overlay.headset.width_m, 0.25);
  assert.equal(opacity.vr_overlay.headset.opacity, 0.1);
});

test("VR position adjustments use desktop steps and clamp every coordinate and rotation", () => {
  for (const kind of ["headset", "wrist"] as const) {
    for (const [name, range] of Object.entries(VR_OVERLAY_POSITION_RANGES[kind])) {
      const field = name as VrOverlayPositionField;
      const branch = settings.vr_overlay[kind];
      const next = applyVrDashboardAction(settings, { adjust_display_position: { kind, field, direction: 1 } });
      assert.deepEqual(next.vr_overlay[kind], { ...branch, [field]: Math.round(((branch as unknown as Record<string, number>)[field] + range.step) * 100) / 100 });
      assert.equal(next.vr_overlay[kind === "headset" ? "wrist" : "headset"], settings.vr_overlay[kind === "headset" ? "wrist" : "headset"]);
      assert.equal(next.translation, settings.translation);
      for (const [boundary, direction] of [[range.min, -1], [range.max, 1]] as const) {
        const edge = { ...settings, vr_overlay: { ...settings.vr_overlay, [kind]: { ...branch, [field]: boundary } } };
        assert.equal(applyVrDashboardAction(edge, { adjust_display_position: { kind, field, direction } }), edge);
      }
    }
  }
  assert.equal(applyVrDashboardAction(settings, { adjust_display_position: { kind: "headset", field: "offset_z_m", direction: 1 } }), settings);
  assert.equal(applyVrDashboardAction(settings, { adjust_display_position: { kind: "wrist", field: "distance_m", direction: 1 } }), settings);
  assert.equal(applyVrDashboardAction(settings, { adjust_display_position: { kind: "headset", field: "pitch_deg", direction: 2 } }), settings);
  assert.equal(applyVrDashboardAction(settings, "headset_width_up").vr_overlay.headset.width_m, 1.25);
  assert.equal(applyVrDashboardAction(settings, "wrist_width_up").vr_overlay.wrist.width_m, 0.33);
});

test("restoring VR position preserves visibility, appearance, sources and the other overlay", () => {
  for (const kind of ["headset", "wrist"] as const) {
    const changed = { ...settings, vr_overlay: { ...settings.vr_overlay, [kind]: { ...settings.vr_overlay[kind], offset_x_m: 0.4, pitch_deg: 45, opacity: 0.4, enabled: false } } };
    const next = applyVrDashboardAction(changed, { reset_display_position: kind });
    assert.equal(next.vr_overlay[kind].offset_x_m, settings.vr_overlay[kind].offset_x_m);
    assert.equal(next.vr_overlay[kind].pitch_deg, settings.vr_overlay[kind].pitch_deg);
    assert.equal(next.vr_overlay[kind].opacity, 0.4);
    assert.equal(next.vr_overlay[kind].enabled, false);
    assert.equal(next.vr_overlay[kind].include_speaker, settings.vr_overlay[kind].include_speaker);
    assert.equal(next.vr_overlay[kind === "headset" ? "wrist" : "headset"], settings.vr_overlay[kind === "headset" ? "wrist" : "headset"]);
    assert.equal(applyVrDashboardAction(settings, { reset_display_position: kind }), settings);
  }
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

test("dashboard OCR toggles change root VR settings without enabling desktop mode", () => {
  const enabled = applyVrDashboardAction(settings, "toggle_ocr");
  const gesture = applyVrDashboardAction(enabled, "toggle_ocr_gesture");
  assert.equal(enabled.ocr.enabled, true);
  assert.equal(enabled.ocr.desktop_enabled, false);
  assert.equal(enabled.vr_overlay, settings.vr_overlay);
  assert.equal(gesture.ocr.hand_gesture_enabled, false);
  assert.equal(gesture.ocr.enabled, true);
});
