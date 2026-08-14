import type {
  ChatboxComposeInput,
  ChatboxPreview,
} from "./types";

export const CHATBOX_LIMIT = 144;

export function createChatboxDraft(targetLanguage = "ja"): ChatboxComposeInput {
  return {
    original: "",
    translation: null,
    source_language: null,
    target_language: targetLanguage,
    send_mode: "bilingual",
    message_format: "original_newline_translation",
    custom_format: null,
    overflow_policy: "block",
  };
}

export function previewChatboxLocally(input: ChatboxComposeInput): ChatboxPreview {
  const original = compact(input.original);
  const translation = compact(input.translation ?? "");
  const text = renderDraft(input, original, translation);
  const charCount = [...text].length;
  const hasRequiredText = input.send_mode === "original"
    ? original.length > 0
    : input.send_mode === "translation"
      ? translation.length > 0
      : original.length > 0 && translation.length > 0;
  const formatValid = input.send_mode !== "bilingual"
    || input.message_format !== "custom"
    || validCustomFormat(input.custom_format ?? "");
  return {
    text,
    char_count: charCount,
    limit: CHATBOX_LIMIT,
    over_limit: charCount > CHATBOX_LIMIT,
    truncated: false,
    sendable: hasRequiredText && formatValid && charCount <= CHATBOX_LIMIT,
  };
}

export function clearSentDraft(input: ChatboxComposeInput): ChatboxComposeInput {
  return { ...input, original: "", translation: null };
}

function renderDraft(
  input: ChatboxComposeInput,
  original: string,
  translation: string,
): string {
  if (input.send_mode === "original") return original;
  if (input.send_mode === "translation") return translation;
  const template = input.message_format === "translation_newline_original"
    ? "{translation}\n{original}"
    : input.message_format === "slash_separated"
      ? "{original} / {translation}"
      : input.message_format === "custom"
        ? input.custom_format ?? ""
        : "{original}\n{translation}";
  return template
    .replace("{original}", original)
    .replace("{translation}", translation);
}

function compact(value: string): string {
  return value.replace(/[\u0000-\u001f\u007f]+/g, " ").trim().replace(/\s+/g, " ");
}

function validCustomFormat(value: string): boolean {
  const remainder = value.replace("{original}", "").replace("{translation}", "");
  return value.length <= 80
    && value.match(/\{original\}/g)?.length === 1
    && value.match(/\{translation\}/g)?.length === 1
    && !/[{}\u0000-\u001f\u007f]/.test(remainder);
}
