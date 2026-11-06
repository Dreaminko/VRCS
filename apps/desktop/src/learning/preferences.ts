import { canonicalLanguageTag } from "../translation/translation-languages.ts";
import type { LearningLevel } from "./types";
import type { ReasoningSetting } from "../translation/translation-thinking";
import type { LearningAiSettings } from "../settings/types";

const LEARNING_PREFERENCES_KEY = "vrcs.learning.preferences.v1";
const PREFERENCES_CHANGED = "vrcs:learning-preferences-changed";

export interface LearningPreferences {
  profileId: string;
  model: string;
  explanationLanguage: string;
  explanationLevel: LearningLevel;
  thinkingEnabled: boolean;
  reasoningControl: ReasoningSetting | null;
}

const DEFAULT_PREFERENCES: LearningPreferences = {
  profileId: "",
  model: "",
  explanationLanguage: "ui",
  explanationLevel: "beginner",
  thinkingEnabled: false,
  reasoningControl: null,
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
      thinkingEnabled: parsed.thinkingEnabled === true,
      reasoningControl: readReasoningControl(parsed.reasoningControl),
    };
  } catch {
    return { ...DEFAULT_PREFERENCES };
  }
}

function readReasoningControl(value: unknown): ReasoningSetting | null {
  if (!value || typeof value !== "object") return null;
  const control = value as { type?: unknown; value?: unknown };
  if (control.type === "effort" && typeof control.value === "string"
    && /^[a-z0-9_]{1,32}$/.test(control.value)) {
    return { type: "effort", value: control.value };
  }
  if (control.type === "budget_tokens" && typeof control.value === "number"
    && Number.isSafeInteger(control.value)
    && (control.value === -1 || (control.value > 0 && control.value <= 4_294_967_295))) {
    return { type: "budget_tokens", value: control.value };
  }
  return null;
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
  const updated = update(readLearningPreferences(serialized));
  let stored: Record<string, unknown> = {};
  try { stored = JSON.parse(serialized ?? "{}") ?? {}; } catch { /* Replace invalid preferences. */ }
  const next = JSON.stringify({ ...stored, explanationLanguage: updated.explanationLanguage, explanationLevel: updated.explanationLevel });
  if (next === serialized) return;
  window.localStorage.setItem(LEARNING_PREFERENCES_KEY, next);
  window.dispatchEvent(new Event(PREFERENCES_CHANGED));
}

export function legacyLearningBinding(serialized: string | null): LearningAiSettings | null {
  const saved = readLearningPreferences(serialized);
  if (!saved.profileId && !saved.model) return null;
  return { profile_id: saved.profileId || null, model: saved.model, use_default_model: false,
    thinking_enabled: saved.thinkingEnabled, reasoning_control: saved.reasoningControl };
}

export function clearLegacyLearningBinding(): void {
  const serialized = learningPreferencesSnapshot();
  if (!serialized) return;
  try {
    const stored = JSON.parse(serialized);
    for (const key of ["profileId", "model", "thinkingEnabled", "reasoningControl"]) delete stored[key];
    window.localStorage.setItem(LEARNING_PREFERENCES_KEY, JSON.stringify(stored));
    window.dispatchEvent(new Event(PREFERENCES_CHANGED));
  } catch { /* Leave unreadable preferences unchanged. */ }
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
