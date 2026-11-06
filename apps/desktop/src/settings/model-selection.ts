import type { LearningAiSettings, ModelBinding, ModelSelection, Settings } from "./types";

export const EMPTY_MODEL_BINDING: ModelBinding = { profile_id: null, model: "" };
export const DEFAULT_MODEL_SELECTION: ModelSelection = { ...EMPTY_MODEL_BINDING, use_default_model: true };
export const DEFAULT_LEARNING_AI: LearningAiSettings = {
  ...DEFAULT_MODEL_SELECTION, thinking_enabled: false, reasoning_control: null,
};

export function resolveModelSelection<T extends ModelSelection>(selection: T, fallback: ModelBinding): T {
  if (!selection.use_default_model) return selection;
  const resolved = { ...selection, profile_id: fallback.profile_id, model: fallback.model };
  if ("reasoning_control" in resolved && (selection.profile_id !== fallback.profile_id || selection.model !== fallback.model)) {
    resolved.reasoning_control = null;
  }
  return resolved;
}

export function translationModelSelection(settings: Settings, ocr = false): ModelSelection {
  return resolveModelSelection(
    (ocr ? settings.ocr.model_selection : settings.translation.model_selection) ?? DEFAULT_MODEL_SELECTION,
    settings.model_defaults?.text ?? EMPTY_MODEL_BINDING,
  );
}

export function learningModelSelection(settings: Settings | null | undefined): LearningAiSettings {
  const selection = settings?.learning_ai ?? DEFAULT_LEARNING_AI;
  return resolveModelSelection(selection, settings?.model_defaults?.text ?? EMPTY_MODEL_BINDING);
}

// Keep optimistic settings aligned with the Core's inherited selections.
export function applyModelDefaults(settings: Settings): Settings {
  const text = settings.model_defaults?.text ?? EMPTY_MODEL_BINDING;
  const vision = settings.model_defaults?.vision ?? EMPTY_MODEL_BINDING;
  const translation = translationModelSelection(settings);
  const ocr = translationModelSelection(settings, true);
  const targets = <T extends ModelSelection>(items: T[], fallback: ModelBinding): T[] => items.map(
    (item) => ({ ...item, ...resolveModelSelection(item, fallback) }),
  );
  return {
    ...settings,
    translation: {
      ...settings.translation, model_selection: translation,
      speaker_targets: targets(settings.translation.speaker_targets, translation),
      microphone_targets: targets(settings.translation.microphone_targets, translation),
    },
    ocr: { ...settings.ocr, model_selection: ocr, targets: targets(settings.ocr.targets, ocr) },
    conversation_titles: { ...settings.conversation_titles, ...resolveModelSelection(settings.conversation_titles, text) },
    vision: { ...settings.vision, ...resolveModelSelection(settings.vision, vision) },
    learning_ai: learningModelSelection(settings),
    language_presets: (settings.language_presets ?? []).map((preset) => ({ ...preset,
      speaker_targets: targets(preset.speaker_targets, translation),
      microphone_targets: targets(preset.microphone_targets, translation),
    })),
  };
}
