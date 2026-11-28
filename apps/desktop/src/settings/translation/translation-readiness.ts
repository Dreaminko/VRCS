import { profileSupportsCapability } from "../../providers/provider-catalog.ts";
import { liveTranslationServiceName } from "../../providers/recognition-services.ts";
import type { ApiProfileView } from "../../providers/types";
import {
  DEFAULT_MODEL_SELECTION,
  translationModelSelection,
  translationSelectionReady,
} from "../model-selection.ts";
import type { Settings } from "../types";

export const TRANSLATION_MODEL_ERROR_CODES = new Set([
  "settings.translation_default_model_missing",
  "settings.translation_profile_missing",
  "settings.translation_model_required",
  "settings.translation_profile_unavailable",
]);

export type TranslationModelIssue =
  | "no_profiles"
  | "default_model_missing"
  | "profile_missing"
  | "model_required"
  | "profile_unavailable"
  | null;

export function errorRequiresTranslationModelSettings(code: string | null | undefined): boolean {
  return Boolean(code && TRANSLATION_MODEL_ERROR_CODES.has(code));
}

export function translationModelIssue(
  settings: Settings,
  apiProfiles: ApiProfileView[],
): TranslationModelIssue {
  const selection = translationModelSelection(settings);
  const configuredTargets = [
    ...settings.translation.speaker_targets,
    ...settings.translation.microphone_targets,
  ];
  const allTargetsReady = configuredTargets.length > 0
    && configuredTargets.every((target) => translationSelectionReady(target, apiProfiles));
  if (translationSelectionReady(selection, apiProfiles) || allTargetsReady) {
    return null;
  }
  if (selection.backend === "plamo_local") {
    return "profile_unavailable";
  }
  const useDefault = (settings.translation.model_selection ?? DEFAULT_MODEL_SELECTION).use_default_model ?? true;
  if (!selection.profile_id) {
    const hasTranslationProfile = apiProfiles.some(
      (profile) => profileSupportsCapability(profile, "text_translation"),
    );
    if (!hasTranslationProfile) return "no_profiles";
    return useDefault ? "default_model_missing" : "profile_missing";
  }
  const profile = apiProfiles.find((item) => item.id === selection.profile_id);
  if (!profile || !profileSupportsCapability(profile, "text_translation")) {
    return "profile_unavailable";
  }
  if (profile.capabilities.supports_model_listing && !selection.model.trim()) {
    return "model_required";
  }
  return null;
}

export function liveTranslationOnlyNeedsModelForManualOrExtraTargets(settings: Settings): boolean {
  return Boolean(liveTranslationServiceName(settings.asr.backend))
    && settings.translation.speaker_targets.length <= 1
    && settings.translation.microphone_targets.length <= 1;
}
