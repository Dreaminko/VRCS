import type { ConversationCatalog, CoreConversation } from "./conversations";
import type {
  AudioLevel,
  LiveTranscription,
  Subtitle,
  SubtitleTranslation,
  VrchatMuteStatus,
} from "./types";

export const SUBTITLE_HISTORY_PAGE_SIZE = 100;
export const MAX_SUBTITLE_HISTORY_ITEMS = 2_000;
export const MAX_SUBTITLE_HISTORY_TEXT_CHARS = 4_000_000;
const MAX_STREAM_TEXT_LENGTH = 100_000;

export interface ConversationSubtitlePage {
  items: Subtitle[];
  has_more: boolean;
  next_before_id: number | null;
}

export interface ParsedConversationSubtitlePage {
  items: Subtitle[];
  hasOlder: boolean;
  nextBeforeId: number | null;
}

export interface ConversationRequestToken {
  conversationId: string;
  version: number;
}

export interface ConversationCatalogEvent {
  sequence: number;
  catalog: ConversationCatalog;
}

export type SubtitleStreamMessage =
  | { type: "subtitle"; subtitle: Subtitle; utterance_id?: string }
  | { type: "conversation_catalog"; catalog: ConversationCatalog }
  | LiveTranscription
  | AudioLevel
  | { type: "vrchat_mute_status"; status: VrchatMuteStatus }
  | {
      type: "failed";
      source?: LiveTranscription["source"];
      code?: string;
      detail?: string;
    }
  | { type: "translation_started"; subtitle_id: number }
  | {
      type: "translation_partial";
      subtitle_id: number;
      text: string;
      target_language: string;
    }
  | {
      type: "translation_completed";
      subtitle_id: number;
      translation: SubtitleTranslation;
    }
  | {
      type: "translation_failed";
      subtitle_id: number;
      code?: string;
      detail?: string;
    };

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isText(value: unknown): value is string {
  return typeof value === "string" && value.length <= MAX_STREAM_TEXT_LENGTH;
}

function isSubtitleId(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) > 0;
}

function isSource(value: unknown): value is LiveTranscription["source"] {
  return value === "speaker" || value === "microphone";
}

function isSubtitleSource(value: unknown): value is NonNullable<Subtitle["source"]> {
  return isSource(value) || value === "chatbox";
}

function isDbfs(value: unknown): value is number {
  return typeof value === "number"
    && Number.isFinite(value)
    && value >= -80
    && value <= 0;
}

function isNullableText(value: unknown): value is string | null {
  return value === null || isText(value);
}

function isNullableFiniteNumber(value: unknown): value is number | null {
  return value === null || (typeof value === "number" && Number.isFinite(value));
}

function isVrchatMuteStatus(value: unknown): value is VrchatMuteStatus {
  return isObject(value)
    && typeof value.enabled === "boolean"
    && ["disabled", "discovering", "connected", "unavailable"].includes(String(value.connection))
    && (value.muted === null || typeof value.muted === "boolean")
    && isNullableText(value.last_error);
}

function isCoreConversation(value: unknown): value is CoreConversation {
  return isObject(value)
    && isText(value.id)
    && isText(value.started_at)
    && isNullableText(value.ended_at)
    && isNullableText(value.automatic_title)
    && isNullableText(value.custom_title)
    && isNullableText(value.icon)
    && Number.isSafeInteger(value.subtitle_count)
    && Number(value.subtitle_count) >= 0
    && isText(value.updated_at)
    && typeof value.active === "boolean";
}

function isConversationCatalog(value: unknown): value is ConversationCatalog {
  return isObject(value)
    && Array.isArray(value.conversations)
    && value.conversations.every(isCoreConversation);
}

function isTranslation(value: unknown): value is SubtitleTranslation {
  return isObject(value)
    && isText(value.text)
    && isNullableText(value.source_language)
    && isText(value.target_language)
    && typeof value.provider === "string"
    && ["alibaba_cloud", "openai", "gemini", "openai_compatible", "deepl", "microsoft_translator", "local"]
      .includes(value.provider)
    && isNullableText(value.model)
    && isText(value.created_at);
}

function isSubtitle(value: unknown): value is Subtitle {
  return isObject(value)
    && (value.id === null || isSubtitleId(value.id))
    && isText(value.text)
    && isNullableText(value.language)
    && isNullableFiniteNumber(value.started_at)
    && isNullableFiniteNumber(value.ended_at)
    && isText(value.created_at)
    && (
      value.conversation_id === undefined
      || value.conversation_id === null
      || isText(value.conversation_id)
    )
    && (value.source === undefined || isSubtitleSource(value.source))
    && Array.isArray(value.translations)
    && value.translations.every(isTranslation)
    && (
      value.translation_partial === undefined
      || (
        isObject(value.translation_partial)
        && isText(value.translation_partial.text)
        && isText(value.translation_partial.target_language)
      )
    );
}

export function parseSubtitleStreamMessage(
  raw: unknown,
): SubtitleStreamMessage | null {
  let value: unknown;
  try {
    value = JSON.parse(String(raw));
  } catch {
    return null;
  }
  if (!isObject(value) || typeof value.type !== "string") return null;
  switch (value.type) {
    case "subtitle":
      return isSubtitle(value.subtitle)
        && (value.utterance_id === undefined || isText(value.utterance_id))
        ? {
            type: "subtitle",
            subtitle: value.subtitle,
            ...(typeof value.utterance_id === "string"
              ? { utterance_id: value.utterance_id }
              : {}),
          }
        : null;
    case "conversation_catalog":
      return isConversationCatalog(value.catalog)
        ? { type: "conversation_catalog", catalog: value.catalog }
        : null;
    case "partial":
      return isSource(value.source)
        && isText(value.text)
        && isText(value.utterance_id)
        && (
          value.language === undefined
          || isNullableText(value.language)
        )
        ? value as SubtitleStreamMessage
        : null;
    case "audio_level":
      return isSource(value.source)
        && isDbfs(value.rms_dbfs)
        && isDbfs(value.peak_dbfs)
        && typeof value.speech === "boolean"
        ? value as SubtitleStreamMessage
        : null;
    case "vrchat_mute_status":
      return isVrchatMuteStatus(value.status)
        ? value as SubtitleStreamMessage
        : null;
    case "failed":
      return (value.source === undefined || isSource(value.source))
        && (value.code === undefined || typeof value.code === "string")
        && (value.detail === undefined || isText(value.detail))
        ? value as SubtitleStreamMessage
        : null;
    case "translation_started":
      return isSubtitleId(value.subtitle_id)
        ? value as SubtitleStreamMessage
        : null;
    case "translation_partial":
      return isSubtitleId(value.subtitle_id)
        && isText(value.text)
        && isText(value.target_language)
        ? value as SubtitleStreamMessage
        : null;
    case "translation_completed":
      return isSubtitleId(value.subtitle_id) && isTranslation(value.translation)
        ? value as SubtitleStreamMessage
        : null;
    case "translation_failed":
      return isSubtitleId(value.subtitle_id)
        && (value.code === undefined || typeof value.code === "string")
        && (value.detail === undefined || isText(value.detail))
        ? value as SubtitleStreamMessage
        : null;
    default:
      return null;
  }
}

export function conversationSubtitlePage(
  page: ConversationSubtitlePage,
): ParsedConversationSubtitlePage {
  return {
    items: page.items.slice(0, SUBTITLE_HISTORY_PAGE_SIZE),
    hasOlder: page.has_more,
    nextBeforeId: page.has_more ? page.next_before_id : null,
  };
}

export function isAbortError(reason: unknown): boolean {
  return isObject(reason) && reason.name === "AbortError";
}

export function isConversationRequestCurrent(
  request: ConversationRequestToken,
  currentConversationId: string | null,
  currentVersion: number,
): boolean {
  return request.conversationId === currentConversationId
    && request.version === currentVersion;
}

function subtitleKey(subtitle: Subtitle): string {
  if (subtitle.id !== null) return `id:${subtitle.id}`;
  return JSON.stringify([
    "ephemeral",
    subtitle.created_at,
    subtitle.source ?? "speaker",
    subtitle.text,
  ]);
}

function newestFirst(left: Subtitle, right: Subtitle): number {
  if (left.id !== null && right.id !== null && left.id !== right.id) {
    return right.id - left.id;
  }
  return (Date.parse(right.created_at) || 0) - (Date.parse(left.created_at) || 0);
}

function translationKey(translation: SubtitleTranslation): string {
  return translation.target_language;
}

function mergeTranslations(
  preferred: SubtitleTranslation[],
  fallback: SubtitleTranslation[],
): SubtitleTranslation[] {
  const merged = new Map<string, SubtitleTranslation>();
  for (const translation of [...preferred, ...fallback]) {
    const key = translationKey(translation);
    if (!merged.has(key)) merged.set(key, translation);
  }
  return [...merged.values()];
}

function subtitleTextLength(subtitle: Subtitle): number {
  return subtitle.text.length
    + (subtitle.translation_partial?.text.length ?? 0)
    + subtitle.translations.reduce((total, translation) => total + translation.text.length, 0);
}

function limitSubtitleHistory(subtitles: Subtitle[]): Subtitle[] {
  let totalTextLength = 0;
  let count = 0;
  for (const subtitle of subtitles) {
    const nextTextLength = totalTextLength + subtitleTextLength(subtitle);
    if (count > 0 && nextTextLength > MAX_SUBTITLE_HISTORY_TEXT_CHARS) break;
    totalTextLength = nextTextLength;
    count += 1;
    if (count >= MAX_SUBTITLE_HISTORY_ITEMS) break;
  }
  return count === subtitles.length ? subtitles : subtitles.slice(0, count);
}

function mergeSubtitle(preferred: Subtitle, fallback: Subtitle): Subtitle {
  const translations = mergeTranslations(
    preferred.translations,
    fallback.translations,
  );
  const translationPartial =
    preferred.translation_partial ?? fallback.translation_partial;
  return {
    ...fallback,
    ...preferred,
    translations,
    translation_partial:
      translationPartial
      && !translations.some(
        (translation) =>
          translation.target_language === translationPartial.target_language,
      )
        ? translationPartial
        : undefined,
  };
}

export function mergeSubtitleHistory(
  preferred: Subtitle[],
  fallback: Subtitle[],
): Subtitle[] {
  const merged = new Map<string, Subtitle>();
  for (const subtitles of [preferred, fallback]) {
    for (const subtitle of subtitles) {
      const key = subtitleKey(subtitle);
      const current = merged.get(key);
      merged.set(key, current ? mergeSubtitle(current, subtitle) : subtitle);
    }
  }
  return limitSubtitleHistory([...merged.values()].sort(newestFirst));
}

export function upsertSubtitleHistory(
  current: Subtitle[],
  subtitle: Subtitle,
): Subtitle[] {
  const key = subtitleKey(subtitle);
  const existingIndex = current.findIndex((item) => subtitleKey(item) === key);
  if (existingIndex >= 0) {
    const next = [...current];
    next[existingIndex] = mergeSubtitle(subtitle, current[existingIndex]);
    return limitSubtitleHistory(next);
  }

  const insertAt = current.findIndex((item) => newestFirst(subtitle, item) < 0);
  const next = [...current];
  next.splice(insertAt < 0 ? next.length : insertAt, 0, subtitle);
  return limitSubtitleHistory(next);
}
