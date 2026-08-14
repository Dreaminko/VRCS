import type { ApiProfile, ApiProfilePurpose } from "./types";

export function apiProfilePurpose(profile: ApiProfile): ApiProfilePurpose {
  if (profile.purpose) return profile.purpose;
  if (
    profile.provider === "deepl"
    || profile.provider === "microsoft_translator"
    || profile.provider === "openai_compatible"
  ) {
    return "llm";
  }
  return "shared";
}

export function supportsRecognition(profile: ApiProfile): boolean {
  const purpose = apiProfilePurpose(profile);
  return (purpose === "asr" || purpose === "shared")
    && (profile.provider === "alibaba_cloud" || profile.provider === "openai");
}

export function supportsTranslation(profile: ApiProfile): boolean {
  const purpose = apiProfilePurpose(profile);
  return purpose === "llm" || purpose === "shared";
}

export function supportsLlmModels(profile: ApiProfile): boolean {
  return supportsTranslation(profile)
    && ["alibaba_cloud", "openai", "openai_compatible"].includes(profile.provider);
}
