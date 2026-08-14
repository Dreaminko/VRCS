import type {
  AudioLevel,
  LiveTranscription,
  Subtitle,
  SubtitleTranslation,
  VrchatMuteStatus,
} from "./types";

const DEFAULT_HISTORY_LIMIT = 500;
const MAX_STREAM_TEXT_LENGTH = 100_000;

export type SubtitleStreamMessage =
  | { type: "subtitle"; subtitle: Subtitle }
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
        ? { type: "subtitle", subtitle: value.subtitle }
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

/**
 * Reconciles a preferred live view with an older or overlapping snapshot.
 * Live fields win, while persisted translations fill gaps that may have been
 * created while the WebSocket was disconnected.
 */
export function mergeSubtitleHistory(
  preferred: Subtitle[],
  fallback: Subtitle[],
  limit = DEFAULT_HISTORY_LIMIT,
): Subtitle[] {
  const merged = new Map<string, Subtitle>();
  for (const subtitle of [...preferred, ...fallback]) {
    const key = subtitleKey(subtitle);
    const current = merged.get(key);
    merged.set(key, current ? mergeSubtitle(current, subtitle) : subtitle);
  }
  return [...merged.values()].sort(newestFirst).slice(0, limit);
}
