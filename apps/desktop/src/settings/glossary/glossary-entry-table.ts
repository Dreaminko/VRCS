import type { GlossaryCategory } from "../../types";
import type { GlossaryEntryDraft } from "./glossary-utils";

export type GlossaryEntryCategoryFilter = GlossaryCategory | "all";
export type GlossaryEntryMappingFilter = "all" | "translated" | "original";
export type GlossaryEntrySortKey = "category" | "position" | "source" | "target";
export type GlossaryEntrySortDirection = "asc" | "desc";
export type GlossaryEntryBulkAction =
  | `category:${GlossaryCategory}`
  | "case:insensitive"
  | "case:sensitive"
  | "target:keep"
  | "target:translate";

export interface GlossaryEntryViewRow {
  entry: GlossaryEntryDraft;
  index: number;
}

export function visibleGlossaryEntries(
  entries: readonly GlossaryEntryDraft[],
  query: string,
  category: GlossaryEntryCategoryFilter,
  mapping: GlossaryEntryMappingFilter,
  sortKey: GlossaryEntrySortKey,
  sortDirection: GlossaryEntrySortDirection,
): GlossaryEntryViewRow[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const rows = entries
    .map((entry, index) => ({ entry, index }))
    .filter(({ entry }) => {
      if (category !== "all" && entry.category !== category) return false;
      if (mapping === "original" && entry.target !== null) return false;
      if (mapping === "translated" && entry.target === null) return false;
      if (!normalizedQuery) return true;
      return `${entry.source}\n${entry.target ?? ""}`.toLocaleLowerCase().includes(normalizedQuery);
    });

  if (sortKey === "position") return rows;
  const direction = sortDirection === "asc" ? 1 : -1;
  return rows.sort((left, right) => {
    const leftValue = sortValue(left.entry, sortKey);
    const rightValue = sortValue(right.entry, sortKey);
    return leftValue.localeCompare(rightValue) * direction || left.index - right.index;
  });
}

export function applyGlossaryEntryBulkAction(
  entries: readonly GlossaryEntryDraft[],
  selectedRowIds: ReadonlySet<string>,
  action: GlossaryEntryBulkAction,
): GlossaryEntryDraft[] {
  return entries.map((entry) => {
    if (!selectedRowIds.has(entry.rowId)) return entry;
    if (action.startsWith("category:")) {
      return { ...entry, category: action.slice("category:".length) as GlossaryCategory };
    }
    if (action === "target:keep") return { ...entry, target: null };
    if (action === "target:translate") return { ...entry, target: entry.target ?? "" };
    return { ...entry, case_sensitive: action === "case:sensitive" };
  });
}

function sortValue(entry: GlossaryEntryDraft, sortKey: Exclude<GlossaryEntrySortKey, "position">): string {
  if (sortKey === "source") return entry.source;
  if (sortKey === "target") return entry.target ?? "";
  return entry.category;
}
