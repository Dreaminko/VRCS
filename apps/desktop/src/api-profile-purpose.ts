import { profileSupportsCapability } from "./provider-catalog.ts";
import type { ApiProfileView } from "./providers/types";

export function supportsRecognition(profile: ApiProfileView): boolean {
  return profileSupportsCapability(profile, "speech_to_text");
}

export function supportsTranslation(profile: ApiProfileView): boolean {
  return profileSupportsCapability(profile, "text_translation");
}

export function supportsLlmModels(profile: ApiProfileView): boolean {
  return profileSupportsCapability(profile, "text_generation")
    && profile.capabilities.supports_model_listing;
}

export function supportsContext(profile: ApiProfileView): boolean {
  return profile.capabilities.supports_context;
}
