import type { Lookup } from "./app/app-types";
import type {
  LearningCardDraft,
  LearningItem,
  LearningItemCreateInput,
  LearningItemKind,
  LearningItemStatus,
  LearningTaskType,
} from "./learning/types";
import type { Subtitle } from "./subtitles/types";
import { canonicalLanguageTag } from "./translation-languages.ts";

export const LEARNING_PAGE_SIZE = 50;

export function explanationLanguageForUiLocale(locale?: string): string {
  return canonicalLanguageTag(locale ?? "") ?? "en-US";
}

export function buildSubtitleLearningCapture(
  subtitles: Subtitle[],
  selectedIds?: Iterable<number>,
  options: { mergeFragments?: boolean } = {},
): LearningItemCreateInput | null {
  const selected = selectedIds ? new Set(selectedIds) : null;
  const ordered = subtitles
    .filter((subtitle) => subtitle.id !== null && (!selected || selected.has(subtitle.id)))
    .sort(compareSubtitlesChronologically);
  if (!ordered.length) return null;

  const languages = new Set(ordered.map((subtitle) => subtitle.language));
  const translations = ordered
    .map((subtitle) => subtitle.translations.at(-1)?.text.trim() ?? "")
    .filter(Boolean);
  const sourceText = options.mergeFragments
    ? combineLearningSubtitleText(ordered)
    : ordered.map((subtitle) => subtitle.text.trim()).filter(Boolean).join("\n");
  if (!sourceText) return null;

  return {
    kind: ordered.length === 1 || options.mergeFragments ? "sentence" : "excerpt",
    source_text: sourceText,
    working_text: sourceText,
    selected_text: null,
    source_translation: translations.length ? translations.join("\n") : null,
    source_language: languages.size === 1 ? ordered[0]?.language ?? null : null,
    source_subtitle_ids: ordered.map((subtitle) => subtitle.id as number),
    dictionary_entries: [],
  };
}

export function buildLookupLearningCapture(lookup: Lookup): LearningItemCreateInput {
  return {
    kind: "word",
    source_text: lookup.context.trim(),
    working_text: lookup.context.trim(),
    selected_text: lookup.term.trim() || null,
    source_translation: lookup.origin?.translation?.trim() || null,
    source_language: lookup.origin?.language ?? lookup.entries[0]?.language ?? null,
    source_subtitle_ids: lookup.origin?.id === null || lookup.origin?.id === undefined
      ? []
      : [lookup.origin.id],
    dictionary_entries: lookup.entries.map((entry) => ({ ...entry })),
  };
}

export function normalizeLearningCardDraft(draft: LearningCardDraft): LearningCardDraft {
  return {
    card_type: draft.card_type,
    term: draft.term.trim(),
    reading: optionalText(draft.reading),
    definition: draft.definition.trim(),
    context: draft.context.trim(),
    dictionary: optionalText(draft.dictionary),
    language: optionalText(draft.language),
  };
}

export function mergeLearningItemPages(
  current: LearningItem[],
  incoming: LearningItem[],
): LearningItem[] {
  const items = new Map<number, LearningItem>();
  for (const item of current) items.set(item.id, item);
  for (const item of incoming) items.set(item.id, item);
  return [...items.values()].sort(compareLearningItemsNewestFirst);
}

export function learningItemMatchesStatus(
  item: LearningItem,
  status: LearningItemStatus | "all",
): boolean {
  return status === "all" || item.status === status;
}

export function learningTaskForKind(kind: LearningItemKind): LearningTaskType {
  if (kind === "word") return "contextual_word_explanation";
  if (kind === "excerpt") return "session_review";
  return "sentence_analysis";
}

export function subtitleLearningKey(subtitle: Subtitle): string {
  return `subtitle:${subtitle.id ?? subtitle.created_at}`;
}

export function subtitleSelectionLearningKey(ids: Iterable<number>): string {
  return `subtitles:${[...ids].sort((left, right) => left - right).join(",")}`;
}

export function lookupLearningKey(lookup: Lookup): string {
  return `lookup:${lookup.origin?.id ?? "none"}:${lookup.term}:${lookup.context}`;
}

export function learningItemCaptureKeys(item: LearningItem): string[] {
  const keys = item.source_subtitle_ids.map((id) => `subtitle:${id}`);
  if (item.source_subtitle_ids.length > 1) {
    keys.push(subtitleSelectionLearningKey(item.source_subtitle_ids));
  }
  if (item.kind === "word") {
    keys.push(`lookup:${item.source_subtitle_ids[0] ?? "none"}:${item.selected_text ?? ""}:${item.source_text}`);
  }
  return keys;
}

function combineLearningSubtitleText(subtitles: Subtitle[]): string {
  const rows = subtitles.map((subtitle) => ({
    text: subtitle.text.trim(),
    language: subtitle.language?.trim().toLowerCase().split(/[-_]/)[0] ?? "",
    source: subtitle.source ?? "speaker",
  })).filter((row) => row.text);
  if (!rows.length) return "";
  const first = rows[0]!;
  const sameSource = rows.every((row) => row.source === first.source);
  const sameLanguage = rows.every((row) => row.language === first.language);
  const separator = sameSource && sameLanguage
    ? first.language === "zh" || first.language === "ja" ? "" : " "
    : "\n";
  return rows.map((row) => row.text).join(separator);
}

function compareSubtitlesChronologically(left: Subtitle, right: Subtitle): number {
  const byTime = new Date(left.created_at).getTime() - new Date(right.created_at).getTime();
  if (byTime !== 0) return byTime;
  return (left.id ?? 0) - (right.id ?? 0);
}

function compareLearningItemsNewestFirst(left: LearningItem, right: LearningItem): number {
  const byTime = new Date(right.created_at).getTime() - new Date(left.created_at).getTime();
  if (byTime !== 0) return byTime;
  return right.id - left.id;
}

function optionalText(value: string | null | undefined): string | null {
  const normalized = value?.trim();
  return normalized ? normalized : null;
}
