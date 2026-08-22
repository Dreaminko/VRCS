import { request } from "../core-client/transport";
import type { TranslationPromptPreview, TranslationPromptSettings } from "./types";

export const translationSettingsApi = {
  previewTranslationPrompt: (
    prompt: TranslationPromptSettings,
    sourceLanguage?: string | null,
    targetLanguage?: string,
  ) => request<TranslationPromptPreview>("/api/translations/prompt-preview", {
    method: "POST",
    body: JSON.stringify({
      prompt,
      source_language: sourceLanguage,
      target_language: targetLanguage,
    }),
  }),
};
