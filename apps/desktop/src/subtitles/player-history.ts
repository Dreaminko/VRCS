import { detectAll } from "tinyld";
import type { LiveTranscription } from "../capture/types";
import type { SpeakerIdentity, Subtitle } from "./types";
import type { ActiveLanguageSession } from "../core-client/types";
import type { TranslationSettings } from "../settings/types";
import { TRANSLATION_LANGUAGES } from "../translation/translation-languages.ts";

const languageNames = new Map(TRANSLATION_LANGUAGES.flatMap(({ code, englishName }) =>
  [englishName, englishName.replace(/\s*\([^)]*\)$/, "")].map((name) => [name.toLowerCase(), code] as const)));
const textLanguages = new Map<string, string | null>();

function inferOriginalLanguage(original: string): string | null {
  // Only the original is sampled. Bound work and cache size for growing live previews.
  const text = original.trim().slice(0, 512);
  if (!/\p{Letter}/u.test(text)) return null;
  if (textLanguages.has(text)) return textLanguages.get(text)!;
  const [first, second] = detectAll(text);
  let language = first && first.accuracy >= 0.6 && first.accuracy - (second?.accuracy ?? 0) >= 0.2
    ? sourceLanguageKey(first.lang) : null;
  // Shared Han characters are insufficient evidence for hiding Japanese or mixed speech.
  if (language === "zh" && /[\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u.test(text)) language = null;
  if (textLanguages.size >= 512) textLanguages.delete(textLanguages.keys().next().value!);
  textLanguages.set(text, language);
  return language;
}

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

export function historyTrackKey(source: string | undefined, speaker?: SpeakerIdentity | null): string | null {
  return (source ?? "speaker") === "speaker" ? speaker?.id ?? "other:unidentified" : null;
}

export function trackVisible(source: string | undefined, speaker: SpeakerIdentity | null | undefined, hidden: readonly string[]): boolean {
  const key = historyTrackKey(source, speaker);
  return key === null || !hidden.includes(key);
}

export function sourceLanguageKey(value: string | null | undefined): string | null {
  const tag = value?.trim().replaceAll("_", "-");
  if (!tag) return null;
  try {
    const language = new Intl.Locale(languageNames.get(tag.toLowerCase()) ?? tag).language;
    return ["auto", "und", "unknown", "mul", "zxx"].includes(language) ? null : language;
  } catch { return null; }
}

export function originalSourceLanguage(original: string, explicit?: string | null, translationSources: readonly (string | null | undefined)[] = []): string | null {
  const language = sourceLanguageKey(explicit);
  if (language) return language;
  const detected = new Set(translationSources.map(sourceLanguageKey)
    .filter((value): value is string => value !== null));
  return detected.size === 1 ? [...detected][0] : detected.size > 1 ? null : inferOriginalLanguage(original);
}

export function subtitleSourceLanguage(subtitle: Subtitle): string | null {
  return originalSourceLanguage(subtitle.text, subtitle.language, subtitle.translations.map(translation => translation.source_language));
}

export function livePartialSourceLanguage(partial: LiveTranscription): string | null {
  return originalSourceLanguage(partial.conversation_preview?.text || partial.text || partial.completed_original || "", partial.language);
}

export function sourceLanguageVisible(source: string | undefined, language: string | null | undefined, hidden: readonly string[] = []): boolean {
  if ((source ?? "speaker") !== "speaker") return true;
  const key = sourceLanguageKey(language);
  return key === null || !hidden.some((value) => sourceLanguageKey(value) === key);
}

export function subtitleVisible(subtitle: Subtitle, hiddenTracks: readonly string[] = [], hiddenSourceLanguages: readonly string[] = []): boolean {
  return trackVisible(subtitle.source, subtitle.speaker, hiddenTracks)
    && (!hiddenSourceLanguages.length || (subtitle.source ?? "speaker") !== "speaker"
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
    if ((subtitle.source ?? "speaker") !== "speaker") continue;
    const speaker = subtitle.speaker ?? { id: "other:unidentified", index: 0 };
    const previous = speakers.get(speaker.id);
    if (!previous || speaker.display_name) speakers.set(speaker.id, speaker);
  }
  return [...speakers.values()];
}
