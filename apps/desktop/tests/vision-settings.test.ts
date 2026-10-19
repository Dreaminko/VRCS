import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_VISION_SETTINGS, patchVision, selectVisionProfile, supportsVision, visionModels, VISION_LIMITS } from "../src/settings/vision-settings.ts";
import { DEFAULT_OCR_SETTINGS, DEFAULT_OCR_WRIST_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import { DEFAULT_FEATURE_SETTINGS } from "../src/settings/feature-availability.ts";
import { applyVrDashboardAction } from "../src/vr-dashboard/settings-actions.ts";
import type { Settings } from "../src/settings/types.ts";
import type { ApiCapability, ApiProfile, ApiProfileView } from "../src/providers/types.ts";

const settings = {
  schema_version: 34,
  conversation_titles: { auto_generate: false, profile_id: null, model: "", output_language: "auto" },
  features: { ...DEFAULT_FEATURE_SETTINGS },
  ocr: { ...DEFAULT_OCR_SETTINGS },
  vision: { ...DEFAULT_VISION_SETTINGS },
} as unknown as Settings;

test("OpenRouter vision choices exclude known text-only models without hiding unknown models", () => {
  const catalog = { models: ["text", "vision", "unknown"], model_details: { text: { input_modalities: ["text"] }, vision: { input_modalities: ["text", "image"] } } };
  assert.deepEqual(visionModels("openrouter", catalog), ["vision", "unknown"]);
  assert.deepEqual(visionModels("openai_compatible", catalog), catalog.models);
});

function profile(provider: string, capability: ApiCapability = "image_understanding"): ApiProfile {
  return { id: "images", name: "Images", provider, enabled_capabilities: [capability] };
}

test("vision defaults are explicit, flat and opt-in", () => {
  assert.equal(DEFAULT_VISION_SETTINGS.enabled, false);
  assert.equal(DEFAULT_VISION_SETTINGS.vr_enabled, true);
  assert.equal(DEFAULT_VISION_SETTINGS.desktop_enabled, false);
  assert.equal(DEFAULT_VISION_SETTINGS.desktop_shortcut, "Ctrl+Alt+V");
  assert.equal(DEFAULT_VISION_SETTINGS.profile_id, null);
  assert.equal(DEFAULT_VISION_SETTINGS.model, "");
  assert.equal(DEFAULT_VISION_SETTINGS.answer_language, "zh-Hans");
  assert.equal(DEFAULT_VISION_SETTINGS.max_capture_seconds, 30);
  assert.equal(DEFAULT_VISION_SETTINGS.asr_finalize_timeout_ms, 10000);
  assert.equal(DEFAULT_VISION_SETTINGS.request_timeout_seconds, 60);
  assert.equal(DEFAULT_VISION_SETTINGS.max_output_tokens, 1024);
  assert.equal(DEFAULT_VISION_SETTINGS.image_max_edge, 1536);
  assert.equal(DEFAULT_VISION_SETTINGS.image_max_bytes, 4194304);
  assert.equal(DEFAULT_VISION_SETTINGS.display_seconds, 30);
  assert.deepEqual(DEFAULT_VISION_SETTINGS.wrist, DEFAULT_OCR_WRIST_SETTINGS);
  assert.notEqual(DEFAULT_VISION_SETTINGS.wrist, DEFAULT_OCR_SETTINGS.wrist);
  for (const [field, { min, max }] of Object.entries(VISION_LIMITS)) {
    const value = DEFAULT_VISION_SETTINGS[field as keyof typeof VISION_LIMITS];
    assert.ok(value >= min && value <= max, field);
  }
});

test("vision profiles with explicit image capability are selectable across supported providers", () => {
  for (const provider of [
    "deepseek", "groq", "alibaba_cloud", "qwen_ai", "openai", "gemini",
    "openai_compatible", "openrouter", "lm_studio", "ollama",
  ]) {
    assert.equal(supportsVision(profile(provider)), true, provider);
    assert.equal(supportsVision(profile(provider, "text_generation")), false, provider);
    assert.equal(supportsVision(profile(provider, "text_translation")), false, provider);
    assert.equal(supportsVision(profile(provider, "speech_to_text")), false, provider);
    assert.equal(supportsVision({ ...profile(provider), enabled_capabilities: [] }), false, provider);
  }
});

test("vision selection uses image capability rather than a provider whitelist", () => {
  assert.equal(supportsVision(profile("future_provider")), true);
  for (const provider of ["qwen_ai", "alibaba_token_plan"]) {
    assert.equal(supportsVision(profile(provider, "text_generation")), false, provider);
  }
});

test("vision catalogs filter known text-only models across providers and retain unknown models", () => {
  const catalog = {
    models: ["text", "vision", "unknown"],
    model_details: {
      text: { input_modalities: ["text"] },
      vision: { input_modalities: ["text", "image"] },
    },
  };
  for (const provider of ["openai", "gemini", "openrouter", "qwen_ai"]) {
    assert.deepEqual(visionModels(provider, catalog), ["vision", "unknown"]);
  }
});

test("vision edits preserve OCR and profile changes do not reuse an unverified model", () => {
  const edited = patchVision(settings, { answer_language: "ja", model: "image-model" });
  assert.equal(edited.ocr, settings.ocr);
  assert.equal(settings.vision.model, "");
  const enabled = patchVision(edited, { enabled: true, profile_id: "old" });
  const selected = selectVisionProfile(enabled, "images");
  const withReasoning = patchVision(enabled, { reasoning_control: { type: "effort", value: "high" } });
  assert.equal(selectVisionProfile(withReasoning, "images").vision.reasoning_control, null);
  assert.equal(selected.vision.profile_id, "images");
  assert.equal(selected.vision.model, "");
  assert.equal(selected.vision.enabled, true);
  assert.equal(selected.vision.answer_language, "ja");
  assert.equal(selectVisionProfile(selected, "images"), selected);
  const cleared = selectVisionProfile(selected, null);
  assert.equal(cleared.vision.profile_id, null);
  assert.equal(cleared.vision.model, "");
  assert.equal(cleared.vision.enabled, true);
  assert.equal(cleared.ocr, settings.ocr);
});

test("common and desktop edits preserve mode preferences without VR or OCR", () => {
  const desktopOnly = {
    ...settings,
    features: { ...settings.features, visual_question_answering: true, vr_overlay: false, ocr: false },
  };
  const enabled = patchVision(desktopOnly, { enabled: true, desktop_enabled: true, desktop_shortcut: "Ctrl+Shift+V" });
  assert.equal(enabled.features, desktopOnly.features);
  assert.equal(enabled.vision.vr_enabled, true);
  assert.equal(enabled.vision.desktop_enabled, true);
  assert.equal(enabled.vision.wrist, settings.vision.wrist);
  const commonOff = patchVision(enabled, { enabled: false });
  assert.equal(commonOff.vision.desktop_enabled, true);
  assert.equal(commonOff.vision.vr_enabled, true);
  assert.equal(commonOff.vision.desktop_shortcut, "Ctrl+Shift+V");
});

test("dashboard vision toggle is independent of desktop and OCR and respects the common master", () => {
  const profiles = [profile("openrouter")] as ApiProfileView[];
  assert.equal(applyVrDashboardAction(settings, "toggle_vision", profiles), settings);
  const available = { ...settings, features: { ...settings.features, visual_question_answering: true, ocr: false } };
  assert.equal(applyVrDashboardAction(available, "toggle_vision", profiles), available);
  const configured = patchVision(available, { enabled: true, vr_enabled: false, desktop_enabled: true, profile_id: "images", model: "image-model" });
  const enabled = applyVrDashboardAction(configured, "toggle_vision", profiles);
  assert.equal(enabled.vision.enabled, true);
  assert.equal(enabled.vision.vr_enabled, true);
  assert.equal(enabled.vision.desktop_enabled, true);
  assert.equal(enabled.ocr, settings.ocr);
  const disabled = applyVrDashboardAction(enabled, "toggle_vision", profiles);
  assert.equal(disabled.vision.vr_enabled, false);
  assert.equal(disabled.vision.enabled, true);
  assert.equal(disabled.vision.desktop_enabled, true);
  const commonOff = patchVision(configured, { enabled: false });
  assert.equal(applyVrDashboardAction(commonOff, "toggle_vision", profiles), commonOff);
  const noVr = { ...configured, features: { ...configured.features, vr_overlay: false } };
  assert.equal(applyVrDashboardAction(noVr, "toggle_vision", profiles), noVr);
});
