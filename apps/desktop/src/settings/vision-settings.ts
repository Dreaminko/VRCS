import type { ApiProfile, ApiModelCatalog } from "../providers/types";
import type { Settings, VisionSettings } from "./types";
import { DEFAULT_OCR_WRIST_SETTINGS } from "./vr-overlay-settings.ts";

export const DEFAULT_VISION_SETTINGS: VisionSettings = {
  enabled: false,
  vr_enabled: true,
  desktop_enabled: false,
  desktop_shortcut: "Ctrl+Alt+V",
  profile_id: null,
  model: "",
  thinking_enabled: false,
  answer_language: "zh-Hans",
  system_prompt: "Answer the user's question about the image concisely. Use the requested answer language. If the image does not provide enough information, say so. Do not invent details.",
  max_capture_seconds: 30,
  asr_finalize_timeout_ms: 10000,
  request_timeout_seconds: 60,
  max_output_tokens: 1024,
  image_max_edge: 1536,
  image_max_bytes: 4194304,
  display_seconds: 30,
  wrist: { ...DEFAULT_OCR_WRIST_SETTINGS },
};

export const VISION_LIMITS = {
  max_capture_seconds: { min: 1, max: 120, step: 1 },
  asr_finalize_timeout_ms: { min: 100, max: 120000, step: 100 },
  request_timeout_seconds: { min: 1, max: 600, step: 1 },
  max_output_tokens: { min: 1, max: 32768, step: 1 },
  image_max_edge: { min: 64, max: 8192, step: 64 },
  image_max_bytes: { min: 1024, max: 20971520, step: 1024 },
  display_seconds: { min: 1, max: 120, step: 1 },
} as const;

// Protocol support does not guarantee that the selected model accepts images.
export function supportsVision(profile: ApiProfile): boolean {
  return profile.enabled_capabilities.includes("image_understanding");
}

export function visionModels(provider: string | undefined, catalog: ApiModelCatalog): string[] {
  if (provider === "openai_compatible") return catalog.models;
  return catalog.models.filter((model) => {
    const details = catalog.model_details?.[model];
    return (!details?.input_modalities?.length || details.input_modalities.includes("image"))
      && (!details?.output_modalities?.length || details.output_modalities.includes("text"));
  });
}

export function patchVision(settings: Settings, patch: Partial<VisionSettings>): Settings {
  return { ...settings, vision: { ...settings.vision, ...patch } };
}

export function selectVisionProfile(settings: Settings, profileId: string | null): Settings {
  if (settings.vision.profile_id === profileId) return settings;
  return patchVision(settings, { profile_id: profileId, model: "", reasoning_control: null });
}
