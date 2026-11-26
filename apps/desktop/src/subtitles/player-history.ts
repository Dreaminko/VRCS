import type { LiveTranscription } from "../capture/types";
import type { SpeakerIdentity, Subtitle } from "./types";
import type { ActiveLanguageSession } from "../core-client/types";
import type { TranslationSettings } from "../settings/types";
import { TRANSLATION_LANGUAGES } from "../translation/translation-languages.ts";

const languageNames = new Map(TRANSLATION_LANGUAGES.flatMap(({ code, englishName }) =>
  [englishName, englishName.replace(/\s*\([^)]*\)$/, "")].map((name) => [name.toLowerCase(), code] as const)));

export function defaultHiddenSourceLanguages(translation?: TranslationSettings, session?: ActiveLanguageSession): string[] {
  const targets = session && session.kind !== "global" ? session.translation.speaker_targets : translation?.speaker_targets ?? [];
  const languages = new Set<string>();
  for (const target of targets) {
    const language = sourceLanguageKey(target.target_language);
    if (language) languages.add(language);
  }
  return [...languages];
}

export interface PlayerHistoryPreferences {
  hiddenTracks: string[];
  /** Legacy null values are treated as an empty selection. */
  hiddenSourceLanguages: string[] | null;
}

export const EMPTY_PLAYER_HISTORY: PlayerHistoryPreferences = { hiddenTracks: [], hiddenSourceLanguages: [] };
export const PLAYER_HISTORY_KEY = "vrcs.player-history.v1";

export function parsePlayerHistory(value: string | null): PlayerHistoryPreferences {
  try {
    const parsed = JSON.parse(value ?? "null") as Partial<PlayerHistoryPreferences> | null;
    const strings = (input: unknown, limit: number) => Array.isArray(input)
      ? [...new Set(input.filter((s): s is string => typeof s === "string" && s.length > 0 && s.length <= 256))].slice(-limit)
      : [];
    return {
      hiddenTracks: strings(parsed?.hiddenTracks, 2000),
      hiddenSourceLanguages: [...new Set(strings(parsed?.hiddenSourceLanguages, 64).map(sourceLanguageKey).filter((language): language is string => language !== null))],
    };
  } catch { return { ...EMPTY_PLAYER_HISTORY }; }
}

export function isAudioTrack(speaker: SpeakerIdentity): boolean {
  return speaker.id.startsWith("vrc:") || /^[0-9a-f-]{36}:\d+:\d+:\d+:\d+$/i.test(speaker.id);
}

export function isOtherPlayerSource(source: string | undefined): boolean {
  return source === undefined || source === "speaker" || source === "vrchat_chatbox";
}

export function historyTrackKey(source: string | undefined, speaker?: SpeakerIdentity | null): string | null {
  return isOtherPlayerSource(source) ? speaker?.id ?? "other:unidentified" : null;
}

export function trackVisible(source: string | undefined, speaker: SpeakerIdentity | null | undefined, hidden: readonly string[]): boolean {
  const key = historyTrackKey(source, speaker);
  return key === null || !hidden.includes(key);
}

const sourceLanguageKeys = new Map<string, string | null>();
const UNSPECIFIED_LANGUAGE_TAGS = new Set(["auto", "und", "unknown", "mul", "zxx"]);

export function sourceLanguageKey(value: string | null | undefined): string | null {
  const tag = value?.trim().replaceAll("_", "-");
  if (!tag) return null;
  if (sourceLanguageKeys.has(tag)) return sourceLanguageKeys.get(tag)!;
  let result: string | null = null;
  try {
    const language = new Intl.Locale(languageNames.get(tag.toLowerCase()) ?? tag).language;
    result = UNSPECIFIED_LANGUAGE_TAGS.has(language) ? null : language;
  } catch {
    result = null;
  }
  if (sourceLanguageKeys.size >= 256) sourceLanguageKeys.clear();
  sourceLanguageKeys.set(tag, result);
  return result;
}

export function originalSourceLanguage(
  explicit?: string | null,
  translationSources: readonly (string | null | undefined)[] = [],
  detectedLanguage?: string | null,
): string | null {
  const language = sourceLanguageKey(explicit);
  if (language) return language;
  const detected = new Set(translationSources.map(sourceLanguageKey)
    .filter((value): value is string => value !== null));
  return detected.size === 1 ? [...detected][0] : detected.size > 1 ? null : sourceLanguageKey(detectedLanguage);
}

export function subtitleSourceLanguage(subtitle: Subtitle): string | null {
  return originalSourceLanguage(
    subtitle.language,
    subtitle.translations.map(translation => translation.source_language),
    subtitle.detected_language,
  );
}

export function livePartialSourceLanguage(partial: LiveTranscription): string | null {
  return originalSourceLanguage(partial.language, [], partial.detected_language);
}

export function sourceLanguageVisible(source: string | undefined, language: string | null | undefined, hidden: readonly string[] = []): boolean {
  if (!isOtherPlayerSource(source)) return true;
  const key = sourceLanguageKey(language);
  return key === null || !hidden.some((value) => sourceLanguageKey(value) === key);
}

export function subtitleVisible(subtitle: Subtitle, hiddenTracks: readonly string[] = [], hiddenSourceLanguages: readonly string[] = []): boolean {
  return trackVisible(subtitle.source, subtitle.speaker, hiddenTracks)
    && (!hiddenSourceLanguages.length || !isOtherPlayerSource(subtitle.source)
      || sourceLanguageVisible(subtitle.source, subtitleSourceLanguage(subtitle), hiddenSourceLanguages));
}

export function livePartialVisible(partial: LiveTranscription, hiddenTracks: readonly string[] = [], hiddenSourceLanguages: readonly string[] = []): boolean {
  return trackVisible(partial.source, partial.speaker, hiddenTracks)
    && (!hiddenSourceLanguages.length || partial.source !== "speaker"
      || sourceLanguageVisible(partial.source, livePartialSourceLanguage(partial), hiddenSourceLanguages));
}

export function historySpeakers(subtitles: readonly Subtitle[]): SpeakerIdentity[] {
  const speakers = new Map<string, SpeakerIdentity>();
  for (const subtitle of subtitles) {
    if (!isOtherPlayerSource(subtitle.source)) continue;
    const speaker = subtitle.speaker ?? { id: "other:unidentified", index: 0 };
    const previous = speakers.get(speaker.id);
    if (!previous || speaker.display_name) speakers.set(speaker.id, speaker);
  }
  return [...speakers.values()];
}
