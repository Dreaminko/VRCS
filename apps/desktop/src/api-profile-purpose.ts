import type { ApiProfile, ApiProfilePurpose, ApiProfileView } from "./types";

export function apiProfilePurpose(profile: ApiProfile): ApiProfilePurpose {
  if (profile.purpose) return profile.purpose;
  if (
    profile.provider === "deepl"
    || profile.provider === "microsoft_translator"
    || profile.provider === "openai_compatible"
    || profile.provider === "gemini"
  ) {
    return "llm";
  }
  return "shared";
}

export function supportsRecognition(profile: ApiProfileView): boolean {
  return profile.capabilities.supports_asr;
}

export function supportsTranslation(profile: ApiProfileView): boolean {
  return profile.capabilities.supports_translation;
}

export function supportsLlmModels(profile: ApiProfileView): boolean {
  return profile.capabilities.supports_model_listing;
}

export function supportsContext(profile: ApiProfileView): boolean {
  return profile.capabilities.supports_context;
}
