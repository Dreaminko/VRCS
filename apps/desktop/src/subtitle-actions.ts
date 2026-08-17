import type { LearningAnalysis, Subtitle } from "./types";

export type SubtitleCopyMode = "original" | "translation" | "bilingual";

export type SubtitleAnalysisOutcome =
  | { status: "completed"; itemId: number; analysis: LearningAnalysis }
  | { status: "opened"; itemId: number };

export function subtitleCopyText(
  original: string,
  translation: string | null,
  mode: SubtitleCopyMode,
): string {
  const normalizedOriginal = original.trim();
  const normalizedTranslation = translation?.trim() ?? "";
  if (mode === "translation") return normalizedTranslation;
  if (mode === "bilingual" && normalizedTranslation) {
    return `${normalizedOriginal}\n${normalizedTranslation}`;
  }
  return normalizedOriginal;
}

export function subtitleSelectionCopyText(
  subtitles: Subtitle[],
  mode: SubtitleCopyMode,
): string {
  const original = combineSubtitleText(subtitles);
  const translation = combineSubtitleTranslations(subtitles);
  return subtitleCopyText(original, translation || null, mode);
}

export function combineSubtitleText(subtitles: Subtitle[]): string {
  const rows = chronologicalSubtitles(subtitles)
    .map((subtitle) => ({
      text: subtitle.text.trim(),
      language: subtitle.language,
      source: subtitle.source ?? "speaker",
    }))
    .filter((row) => row.text);
  return combineRows(rows);
}

function combineSubtitleTranslations(subtitles: Subtitle[]): string {
  const rows = chronologicalSubtitles(subtitles)
    .map((subtitle) => {
      const translation = subtitle.translations.at(-1);
      return translation
        ? { text: translation.text.trim(), language: translation.target_language, source: subtitle.source ?? "speaker" }
        : null;
    })
    .filter((row): row is NonNullable<typeof row> => Boolean(row?.text));
  return combineRows(rows);
}

function combineRows(rows: Array<{ text: string; language: string | null; source: string }>): string {
  if (!rows.length) return "";
  const first = rows[0]!;
  const sameSource = rows.every((row) => row.source === first.source);
  const sameLanguage = rows.every((row) => languageFamily(row.language) === languageFamily(first.language));
  if (!sameSource || !sameLanguage) return rows.map((row) => row.text).join("\n");
  return rows.map((row) => row.text).join(compactScript(first.language) ? "" : " ");
}

function chronologicalSubtitles(subtitles: Subtitle[]): Subtitle[] {
  return [...subtitles].sort((left, right) => {
    const byTime = new Date(left.created_at).getTime() - new Date(right.created_at).getTime();
    return byTime || (left.id ?? 0) - (right.id ?? 0);
  });
}

function languageFamily(language: string | null): string {
  return language?.trim().toLowerCase().split(/[-_]/)[0] ?? "";
}

function compactScript(language: string | null): boolean {
  const family = languageFamily(language);
  return family === "zh" || family === "ja";
}
