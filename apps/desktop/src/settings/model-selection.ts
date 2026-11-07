import { profileSupportsCapability } from "../providers/provider-catalog.ts";
import { liveTranslationServiceName } from "../providers/recognition-services.ts";
import type { ApiProfileView } from "../providers/types";
import {
  PLAMO_LOCAL_MODEL_ID,
  PLAMO_TRANSLATION_LANGUAGE_CODES,
  supportsPlamoTranslationLanguage,
} from "../translation/translation-languages.ts";
import type { LearningAiSettings, ModelBinding, ModelSelection, Settings } from "./types";

export const EMPTY_MODEL_BINDING: ModelBinding = { backend: "api", profile_id: null, model: "" };
export const DEFAULT_MODEL_SELECTION: ModelSelection = { ...EMPTY_MODEL_BINDING, use_default_model: true };
export const DEFAULT_LEARNING_AI: LearningAiSettings = {
  ...DEFAULT_MODEL_SELECTION, thinking_enabled: false, reasoning_control: null,
};

export function resolveModelSelection<T extends ModelSelection>(selection: T, fallback: ModelBinding): T {
  if (!selection.use_default_model) return selection;
  const resolved = {
    ...selection,
    backend: fallback.backend ?? "api",
    profile_id: fallback.profile_id,
    model: fallback.model,
  };
  if ("reasoning_control" in resolved && (
    (selection.backend ?? "api") !== (fallback.backend ?? "api")
    || selection.profile_id !== fallback.profile_id
    || selection.model !== fallback.model
    || resolved.backend === "plamo_local"
  )) {
    resolved.reasoning_control = null;
  }
  if ("thinking_enabled" in resolved && resolved.backend === "plamo_local") {
    resolved.thinking_enabled = false;
  }
  return resolved;
}

export function translationModelSelection(settings: Settings, ocr = false): ModelSelection {
  return resolveModelSelection(
    (ocr ? settings.ocr.model_selection : settings.translation.model_selection) ?? DEFAULT_MODEL_SELECTION,
    settings.model_defaults?.translation ?? settings.model_defaults?.text ?? EMPTY_MODEL_BINDING,
  );
}

export function translationSelectionReady(selection: ModelBinding, profiles: ApiProfileView[]): boolean {
  if (selection.backend === "plamo_local") {
    return selection.profile_id === null && selection.model === PLAMO_LOCAL_MODEL_ID;
  }
  const profile = profiles.find((item) => item.id === selection.profile_id);
  return Boolean(profile && profileSupportsCapability(profile, "text_translation")
    && (!profile.capabilities.supports_model_listing || selection.model.trim()));
}

export function translationPromptBackend(
  routes: Pick<ModelBinding, "backend">[],
): "api" | "plamo_local" {
  return routes.some((route) => (route.backend ?? "api") !== "plamo_local")
    ? "api"
    : "plamo_local";
}

export function learningModelSelection(settings: Settings | null | undefined): LearningAiSettings {
  const selection = settings?.learning_ai ?? DEFAULT_LEARNING_AI;
  return resolveModelSelection(selection, settings?.model_defaults?.text ?? EMPTY_MODEL_BINDING);
}

function withoutBackend<T extends object>(selection: T): T {
  const apiSelection = { ...selection } as T & { backend?: unknown };
  delete apiSelection.backend;
  return apiSelection;
}

// Keep optimistic settings aligned with the Core's inherited selections.
export function applyModelDefaults(settings: Settings): Settings {
  const text = settings.model_defaults?.text ?? EMPTY_MODEL_BINDING;
  const translationDefault = settings.model_defaults?.translation
    ?? settings.model_defaults?.text ?? EMPTY_MODEL_BINDING;
  const vision = settings.model_defaults?.vision ?? EMPTY_MODEL_BINDING;
  const translation = resolveModelSelection(
    settings.translation.model_selection ?? DEFAULT_MODEL_SELECTION,
    translationDefault,
  );
  const ocr = translationModelSelection(settings, true);
  const targets = <T extends ModelSelection & { target_language: string }>(
    items: T[],
    fallback: ModelBinding,
    liveFirst = false,
  ): T[] => {
    const preserveFirst = liveFirst && Boolean(liveTranslationServiceName(settings.asr?.backend));
    const hasSharedBinding = fallback.backend === "plamo_local"
      || Boolean(fallback.profile_id || fallback.model.trim());
    const resolved = items.map((item, index) => preserveFirst && index === 0
      ? { ...item, use_default_model: true }
      : !hasSharedBinding
        ? { ...item, use_default_model: true }
      : resolveModelSelection({ ...item, use_default_model: true }, fallback));
    const used = new Set(resolved
      .filter((item, index) => (preserveFirst && index === 0)
        || item.backend !== "plamo_local"
        || supportsPlamoTranslationLanguage(item.target_language))
      .map((item) => item.target_language.toLowerCase()));
    return resolved.map((item, index) => {
      if (preserveFirst && index === 0) return item;
      if (item.backend !== "plamo_local"
        || supportsPlamoTranslationLanguage(item.target_language)) return item;
      const targetLanguage = PLAMO_TRANSLATION_LANGUAGE_CODES.find(
        (code) => !used.has(code.toLowerCase()),
      ) ?? "en";
      used.add(targetLanguage.toLowerCase());
      return { ...item, target_language: targetLanguage };
    });
  };
  return {
    ...settings,
    translation: {
      ...settings.translation, model_selection: translation,
      speaker_targets: targets(settings.translation.speaker_targets, translation, true),
      microphone_targets: targets(settings.translation.microphone_targets, translation, true),
    },
    ocr: { ...settings.ocr, model_selection: ocr, targets: targets(settings.ocr.targets, ocr) },
    conversation_titles: withoutBackend({
      ...settings.conversation_titles,
      ...resolveModelSelection(settings.conversation_titles, text),
    }),
    vision: withoutBackend({
      ...settings.vision,
      ...resolveModelSelection(settings.vision, vision),
    }),
    learning_ai: withoutBackend(learningModelSelection(settings)),
    language_presets: (settings.language_presets ?? []).map((preset) => ({ ...preset,
      speaker_targets: targets(preset.speaker_targets, translation, true),
      microphone_targets: targets(preset.microphone_targets, translation, true),
    })),
  };
}
