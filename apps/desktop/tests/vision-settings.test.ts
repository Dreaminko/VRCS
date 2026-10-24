import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_VISION_SETTINGS, patchVision, selectVisionProfile, supportsVision, visionModels, VISION_LIMITS,
  selectVisionAsrService, selectVisionAsrSource, visionAsrProfiles, visionAsrServices,
} from "../src/settings/vision-settings.ts";
import { DEFAULT_OCR_SETTINGS, DEFAULT_OCR_WRIST_SETTINGS } from "../src/settings/vr-overlay-settings.ts";
import { DEFAULT_FEATURE_SETTINGS } from "../src/settings/feature-availability.ts";
import { applyVrDashboardAction } from "../src/vr-dashboard/settings-actions.ts";
import type { Settings } from "../src/settings/types.ts";
import type { ApiCapability, ApiProfile, ApiProfileView, AsrSettings, ProviderDefinition, ProviderServiceDefinition } from "../src/providers/types.ts";

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
  assert.equal(DEFAULT_VISION_SETTINGS.asr, null);
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

const questionProfile: ApiProfileView = {
  ...profile("alibaba_cloud", "speech_to_text"), id: "speech", name: "Speech",
  provider_display_name: "Alibaba Cloud", active: true, translation_active: false,
  credential: { configured: true, stored_configured: true, environment_override: false, source: "credential_manager" },
  capabilities: { supports_streaming: true, supports_model_listing: false, requires_api_key: true, is_local: false,
    supports_context: true, supports_translation: false, supports_asr: true, supports_text_generation: false,
    supports_image_understanding: false, supports_custom_translation_language: false, supported_languages: [] },
  support_levels: { asr: "native", translation: null },
};
function questionService(id: string, adapter: string, languages?: string[]): ProviderServiceDefinition {
  return { id, display_name: id, adapter, capabilities: ["speech_to_text"], recognition_transport: "realtime_stream",
    recognition_languages: languages, partial_results: true, models: [id + "-model"], model_listing: false, supports_context: false };
}
const questionDefinitions: ProviderDefinition[] = [{
  id: "alibaba_cloud", display_name: "Alibaba Cloud", category: "cloud_provider",
  connection: { base_url: { mode: "fixed" }, auth_modes: ["bearer"], default_auth_mode: "bearer", fields: [] },
  services: [questionService("qwen_live_translate", "qwen_live_translate"),
    questionService("qwen_realtime", "qwen_realtime"), questionService("fun_asr_realtime", "fun_asr_realtime")],
  capabilities: questionProfile.capabilities, support_levels: questionProfile.support_levels,
}];
const sharedAsr: AsrSettings = {
  backend: "qwen_live_translate", active_profile_id: "speech", language: "ja",
  managed_qwen: { package_id: "qwen3-asr-0.6b-q8_0", device: "cpu" },
  service_settings: { qwen_live_translate: { model: "global-translation-model", context: "" },
    qwen_realtime: { model: "saved-recognition-model", context: "shared context" } },
};

test("question ASR choices exclude translation and services without question completion", () => {
  const unsupported = { ...questionDefinitions[0], id: "stream-only", services: [
    questionService("openai_realtime_translate", "openai_realtime_translate"),
    questionService("gemini_live_translate", "gemini_live_translate"),
    questionService("openai_realtime", "openai_realtime"),
    questionService("gemini_transcribe", "gemini_transcription"),
  ] };
  const profiles = [questionProfile, { ...questionProfile, id: "stream", provider: unsupported.id },
    { ...questionProfile, id: "text", enabled_capabilities: ["text_generation"] as ApiCapability[] }];
  const definitions = [...questionDefinitions, unsupported];
  assert.deepEqual(visionAsrProfiles(profiles, definitions), [questionProfile]);
  assert.deepEqual(visionAsrServices(questionProfile, definitions).map(service => service.id), ["qwen_realtime", "fun_asr_realtime"]);
});

test("independent question ASR reuses an API profile without changing global translation or vision settings", () => {
  const original = { ...settings, asr: sharedAsr };
  const saved = structuredClone(original);
  const asr = selectVisionAsrSource(null, "speech", sharedAsr, [questionProfile], questionDefinitions);
  assert.deepEqual(asr, { backend: "qwen_realtime", profile_id: "speech", language: "ja", model: "saved-recognition-model" });
  const updated = patchVision(original, { asr });
  assert.equal(updated.asr, sharedAsr);
  assert.equal(updated.ocr, original.ocr);
  assert.equal(updated.vision.model, original.vision.model);
  assert.equal(updated.vision.profile_id, original.vision.profile_id);
  assert.deepEqual(original, saved);
  assert.deepEqual(selectVisionAsrSource(asr, "managed_qwen", sharedAsr, [], []), {
    backend: "qwen_local_managed", profile_id: null, language: "ja", model: "",
  });
  assert.equal(selectVisionAsrSource(asr, "", sharedAsr, [], []), null);
});

test("question ASR service changes reset unsupported languages and retain custom models on reselection", () => {
  const mimo = questionService("mimo_transcription", "mimo_transcription", ["auto", "zh", "en"]);
  const current = { backend: "qwen_realtime", profile_id: "speech", language: "ja" as const, model: "custom-question-model" };
  assert.deepEqual(selectVisionAsrService(current, questionDefinitions[0].services[1], sharedAsr), current);
  assert.deepEqual(selectVisionAsrService(current, mimo, sharedAsr), {
    ...current, backend: "mimo_transcription", language: "auto", model: "mimo_transcription-model",
  });
});
