import type { SubtitleSearchHit } from "./subtitles/types";

export function isSubtitleSearchable(query: string): boolean {
  return query.trim().length > 0;
}

export function mergeSubtitleSearchHits(
  current: SubtitleSearchHit[],
  incoming: SubtitleSearchHit[],
): SubtitleSearchHit[] {
  const merged = new Map<string, SubtitleSearchHit>();
  for (const hit of [...current, ...incoming]) {
    const key = `${hit.subtitle.conversation_id ?? ""}:${hit.subtitle.id ?? hit.subtitle.created_at}`;
    if (!merged.has(key)) merged.set(key, hit);
  }
  return [...merged.values()];
}

export function subtitleSearchMatchRange(
  text: string,
  query: string,
): { start: number; end: number } | null {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const start = text.toLocaleLowerCase().indexOf(normalizedQuery);
  if (!normalizedQuery || start < 0) return null;
  return { start, end: start + query.trim().length };
}
