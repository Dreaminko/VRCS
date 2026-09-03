import { canonicalLanguageTag } from "../translation-languages.ts";
import type { LearningLevel } from "./types";

const LEARNING_PREFERENCES_KEY = "vrcs.learning.preferences.v1";
const PREFERENCES_CHANGED = "vrcs:learning-preferences-changed";

export interface LearningPreferences {
  profileId: string;
  model: string;
  explanationLanguage: string;
  explanationLevel: LearningLevel;
}

const DEFAULT_PREFERENCES: LearningPreferences = {
  profileId: "",
  model: "",
  explanationLanguage: "ui",
  explanationLevel: "beginner",
};

export function learningPreferencesSnapshot(): string | null {
  try {
    return window.localStorage.getItem(LEARNING_PREFERENCES_KEY);
  } catch {
    return null;
  }
}

export function readLearningPreferences(serialized: string | null): LearningPreferences {
  try {
    const parsed = JSON.parse(serialized ?? "null") as Partial<LearningPreferences> | null;
    if (!parsed) return { ...DEFAULT_PREFERENCES };
    return {
      profileId: typeof parsed.profileId === "string" ? parsed.profileId : "",
      model: typeof parsed.model === "string" ? parsed.model : "",
      explanationLanguage: typeof parsed.explanationLanguage === "string"
        && parsed.explanationLanguage !== "ui"
        ? canonicalLanguageTag(parsed.explanationLanguage) ?? "ui"
        : "ui",
      explanationLevel: normalizeLearningLevel(parsed.explanationLevel),
    };
  } catch {
    return { ...DEFAULT_PREFERENCES };
  }
}

export function normalizeLearningLevel(value: unknown): LearningLevel {
  if (value === "beginner" || value === "intermediate" || value === "advanced") return value;
  if (value === "brief") return "beginner";
  if (value === "standard") return "intermediate";
  if (value === "detailed") return "advanced";
  return DEFAULT_PREFERENCES.explanationLevel;
}

export function updateLearningPreferences(
  update: (current: LearningPreferences) => LearningPreferences,
): void {
  const serialized = learningPreferencesSnapshot();
  const next = JSON.stringify(update(readLearningPreferences(serialized)));
  if (next === serialized) return;
  window.localStorage.setItem(LEARNING_PREFERENCES_KEY, next);
  window.dispatchEvent(new Event(PREFERENCES_CHANGED));
}

export function subscribeLearningPreferences(listener: () => void): () => void {
  const onStorage = (event: StorageEvent) => {
    if (event.key === null || event.key === LEARNING_PREFERENCES_KEY) listener();
  };
  window.addEventListener(PREFERENCES_CHANGED, listener);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(PREFERENCES_CHANGED, listener);
    window.removeEventListener("storage", onStorage);
  };
}
