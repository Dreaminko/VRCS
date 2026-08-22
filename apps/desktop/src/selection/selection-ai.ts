import type { SelectionTarget } from "../app/app-types";
import type { LearningPreferences } from "../learning/hooks/useLearningWorkspace";
import type { SelectionQueryInput } from "../learning/types";

export function selectionAiConfigured(preferences: LearningPreferences): boolean {
  return Boolean(preferences.profileId && preferences.model.trim());
}

export function selectionQueryInput(
  target: SelectionTarget,
  question: string,
  preferences: LearningPreferences,
): SelectionQueryInput {
  return {
    selected_text: target.selectedText,
    source_text: target.context,
    source_translation: target.origin?.translation ?? null,
    source_language: target.origin?.language ?? null,
    question: question.trim(),
    profile_id: preferences.profileId,
    model: preferences.model.trim(),
    explanation_language: preferences.explanationLanguage,
    level: preferences.explanationLevel,
  };
}
