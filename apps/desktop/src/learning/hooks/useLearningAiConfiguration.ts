import { useMemo, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";

import { explanationLanguageForUiLocale } from "../learning";
import { learningModelSelection } from "../../settings/model-selection";
import type { Settings } from "../../settings/types";
import {
  learningPreferencesSnapshot, normalizeLearningLevel, readLearningPreferences,
  subscribeLearningPreferences, updateLearningPreferences as setPreferences,
} from "../preferences";

export type { LearningPreferences } from "../preferences";

export function useLearningAiConfiguration(settings: Settings | null | undefined) {
  const { i18n } = useTranslation();
  const serialized = useSyncExternalStore(subscribeLearningPreferences, learningPreferencesSnapshot);
  const saved = useMemo(() => readLearningPreferences(serialized), [serialized]);
  const binding = learningModelSelection(settings);
  return {
    preferences: {
      ...saved,
      profileId: binding.profile_id ?? "",
      model: binding.model,
      thinkingEnabled: binding.thinking_enabled,
      reasoningControl: binding.reasoning_control ?? null,
      explanationLanguage: saved.explanationLanguage === "ui"
        ? explanationLanguageForUiLocale(i18n.resolvedLanguage) : saved.explanationLanguage,
    },
    explanationLanguagePreference: saved.explanationLanguage,
    setExplanationLanguage: (explanationLanguage: string) => setPreferences((current) => ({ ...current, explanationLanguage })),
    setExplanationLevel: (explanationLevel: string) => setPreferences((current) => ({
      ...current, explanationLevel: normalizeLearningLevel(explanationLevel),
    })),
  };
}
