import type {
  ChatboxComposeInput,
  ChatboxOutputTarget,
  ChatboxOverflowPolicy,
  ChatboxPreview,
  ChatboxSendMode,
} from "./types";
import { canonicalLanguageTag } from "../translation/translation-languages.ts";

export const CHATBOX_LIMIT = 144;
const SEND_MODES = new Set<ChatboxSendMode>(["original", "translation", "bilingual"]);
const OUTPUT_TARGETS = new Set<ChatboxOutputTarget>(["osc", "unitext", "both"]);
const OVERFLOW_POLICIES = new Set<ChatboxOverflowPolicy>(["block", "smart_truncate"]);

export interface ChatboxPreferences {
  follow_translation_settings: boolean;
  target_language: string;
  send_mode: ChatboxSendMode;
  output_target: ChatboxOutputTarget;
  overflow_policy: ChatboxOverflowPolicy;
}

export interface ChatboxDraft extends ChatboxComposeInput {
  follow_translation_settings: boolean;
  target_language: string;
  output_target: ChatboxOutputTarget;
}

export function createChatboxDraft(targetLanguage = "ja"): ChatboxDraft {
  return {
    original: "",
    translation: null,
    translation_origin: null,
    source_language: null,
    follow_translation_settings: true,
    target_language: targetLanguage,
    send_mode: "bilingual",
    output_target: "both",
    message_format: "original_newline_translation",
    custom_format: null,
    overflow_policy: "block",
  };
}

export function previewChatboxLocally(
  input: ChatboxComposeInput,
  options: { enforceOscLimit?: boolean } = {},
): ChatboxPreview {
  const enforceOscLimit = options.enforceOscLimit ?? true;
  const original = compact(input.original);
  const translation = compact(input.translation ?? "");
  const effectiveInput = enforceOscLimit
    ? input
    : { ...input, message_format: "original_newline_translation" as const, custom_format: null };
  const text = renderDraft(effectiveInput, original, translation);
  const charCount = [...text].length;
  const hasRequiredText = input.send_mode === "original"
    ? original.length > 0
    : input.send_mode === "translation"
      ? translation.length > 0
      : original.length > 0 && translation.length > 0;
  if (!enforceOscLimit) {
    return {
      text,
      char_count: charCount,
      limit: null,
      over_limit: false,
      truncated: false,
      sendable: hasRequiredText,
    };
  }
  const formatValid = input.send_mode !== "bilingual"
    || input.message_format !== "custom"
    || isValidChatboxCustomFormat(input.custom_format ?? "");
  const overLimit = charCount > CHATBOX_LIMIT;
  const truncated = overLimit && input.overflow_policy === "smart_truncate";
  return {
    text,
    char_count: charCount,
    limit: CHATBOX_LIMIT,
    over_limit: overLimit,
    truncated,
    sendable: hasRequiredText && formatValid && (!overLimit || truncated),
  };
}

export function clearSentDraft(input: ChatboxDraft): ChatboxDraft {
  return {
    ...input,
    original: "",
    translation: null,
    translation_origin: null,
    source_language: null,
  };
}

export function updateChatboxTranslation(
  input: ChatboxDraft,
  translation: string,
): ChatboxDraft {
  return {
    ...input,
    translation: translation || null,
    translation_origin: null,
  };
}

export function chatboxPreferencesFromDraft(draft: ChatboxDraft): ChatboxPreferences {
  return {
    follow_translation_settings: draft.follow_translation_settings,
    target_language: normalizeTargetLanguage(draft.target_language, "ja"),
    send_mode: draft.send_mode,
    output_target: draft.output_target,
    overflow_policy: draft.overflow_policy,
  };
}

export function applyChatboxPreferences(
  draft: ChatboxComposeInput,
  preferences: ChatboxPreferences,
): ChatboxDraft {
  return { ...draft, ...preferences };
}

export function normalizeChatboxPreferences(
  value: unknown,
  fallbackTarget: string = "ja",
): ChatboxPreferences {
  const safeFallback = normalizeTargetLanguage(fallbackTarget, "ja");
  const defaults = createChatboxDraft(safeFallback);
  const candidate = parseStoredValue(value);
  if (!candidate || typeof candidate !== "object") {
    return chatboxPreferencesFromDraft(defaults);
  }
  const stored = candidate as Record<string, unknown>;
  const manualTarget = normalizeTargetLanguage(stored.target_language, "");
  return {
    // Older preferences stored a fixed language or null for follow mode.
    follow_translation_settings: typeof stored.follow_translation_settings === "boolean"
      ? stored.follow_translation_settings
      : !manualTarget,
    target_language: manualTarget || safeFallback,
    send_mode: isSetValue(stored.send_mode, SEND_MODES) ? stored.send_mode : defaults.send_mode,
    output_target: isSetValue(stored.output_target, OUTPUT_TARGETS)
      ? stored.output_target
      : defaults.output_target,
    overflow_policy: isSetValue(stored.overflow_policy, OVERFLOW_POLICIES)
      ? stored.overflow_policy
      : defaults.overflow_policy,
  };
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
  return template.replace(
    /\{original\}|\{translation\}/g,
    (token) => (token === "{original}" ? original : translation),
  );
}

function compact(value: string): string {
  return value.replace(/[\u0000-\u001f\u007f]+/g, " ").trim().replace(/\s+/g, " ");
}

export function isValidChatboxCustomFormat(value: string): boolean {
  const remainder = value.replace("{original}", "").replace("{translation}", "");
  return [...value].length <= 200
    && value.match(/\{original\}/g)?.length === 1
    && value.match(/\{translation\}/g)?.length === 1
    && !/[{}\u0000-\u001f\u007f]/.test(remainder);
}

function normalizeTargetLanguage(
  value: unknown,
  fallback: string,
): string {
  return typeof value === "string" ? canonicalLanguageTag(value) ?? fallback : fallback;
}

function isSetValue<T extends string>(value: unknown, values: Set<T>): value is T {
  return typeof value === "string" && values.has(value as T);
}

function parseStoredValue(value: unknown): unknown {
  if (typeof value !== "string") return value;
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}
