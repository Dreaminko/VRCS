import type { ApiProfile, ApiProfilePurpose } from "./types";

export function apiProfilePurpose(profile: ApiProfile): ApiProfilePurpose {
  if (profile.purpose) return profile.purpose;
  if (
    profile.provider === "deepl"
    || profile.provider === "microsoft_translator"
    || (profile.provider === "openai" && Boolean(profile.base_url))
  ) {
    return "llm";
  }
  return "shared";
}

export function supportsRecognition(profile: ApiProfile): boolean {
  const purpose = apiProfilePurpose(profile);
  return (purpose === "asr" || purpose === "shared")
    && (profile.provider === "alibaba_cloud" || (profile.provider === "openai" && !profile.base_url));
}

export function supportsTranslation(profile: ApiProfile): boolean {
  const purpose = apiProfilePurpose(profile);
  return purpose === "llm" || purpose === "shared";
}

export function supportsLlmModels(profile: ApiProfile): boolean {
  return supportsTranslation(profile)
    && (profile.provider === "alibaba_cloud" || profile.provider === "openai");
}
