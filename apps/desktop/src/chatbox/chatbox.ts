import type {
  ChatboxComposeInput,
  ChatboxMessageFormat,
  ChatboxOverflowPolicy,
  ChatboxPreview,
  ChatboxSendMode,
  TranslationSettings,
} from "../types";

export const CHATBOX_LIMIT = 144;
const SEND_MODES = new Set<ChatboxSendMode>(["original", "translation", "bilingual"]);
const MESSAGE_FORMATS = new Set<ChatboxMessageFormat>([
  "original_newline_translation",
  "translation_newline_original",
  "slash_separated",
  "custom",
]);
const OVERFLOW_POLICIES = new Set<ChatboxOverflowPolicy>(["block", "smart_truncate"]);

export interface ChatboxPreferences {
  target_language: TranslationSettings["target_language"];
  send_mode: ChatboxSendMode;
  message_format: ChatboxMessageFormat;
  custom_format: string | null;
  overflow_policy: ChatboxOverflowPolicy;
}

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

export function chatboxPreferencesFromDraft(draft: ChatboxComposeInput): ChatboxPreferences {
  return {
    target_language: normalizeTargetLanguage(draft.target_language, "ja"),
    send_mode: draft.send_mode,
    message_format: draft.message_format,
    custom_format: draft.custom_format,
    overflow_policy: draft.overflow_policy,
  };
}

export function applyChatboxPreferences(
  draft: ChatboxComposeInput,
  preferences: ChatboxPreferences,
): ChatboxComposeInput {
  return { ...draft, ...preferences };
}

export function normalizeChatboxPreferences(
  value: unknown,
  fallbackTarget: TranslationSettings["target_language"] = "ja",
): ChatboxPreferences {
  const defaults = createChatboxDraft(fallbackTarget);
  const candidate = parseStoredValue(value);
  if (!candidate || typeof candidate !== "object") {
    return chatboxPreferencesFromDraft(defaults);
  }
  const stored = candidate as Record<string, unknown>;
  return {
    target_language: normalizeTargetLanguage(stored.target_language, fallbackTarget),
    send_mode: isSetValue(stored.send_mode, SEND_MODES) ? stored.send_mode : defaults.send_mode,
    message_format: isSetValue(stored.message_format, MESSAGE_FORMATS)
      ? stored.message_format
      : defaults.message_format,
    custom_format: typeof stored.custom_format === "string"
      ? Array.from(stored.custom_format).slice(0, 200).join("") || null
      : null,
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

function normalizeTargetLanguage(
  value: unknown,
  fallback: TranslationSettings["target_language"],
): TranslationSettings["target_language"] {
  if (typeof value !== "string") return fallback;
  const input = value.trim();
  if (input.length < 2 || input.length > 35) return fallback;
  if (!/^[A-Za-z]{2,8}(?:-[A-Za-z0-9]{2,8})*$/.test(input)) return fallback;
  try {
    return Intl.getCanonicalLocales(input)[0] ?? fallback;
  } catch {
    return fallback;
  }
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
