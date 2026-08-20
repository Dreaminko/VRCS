import type { DictionaryEntry, Subtitle } from "../types";
import type { LookupAnchor } from "../shared/lib/popover-placement";

export type Page = "live" | "learning" | "settings";

export type LookupOrigin = {
  id: number | null;
  language: string | null;
  source: Subtitle["source"] | null;
  createdAt: string;
  translation: string | null;
};

export type SelectionTarget = {
  selectedText: string;
  context: string;
  origin?: LookupOrigin;
  anchor: LookupAnchor;
  range: Range;
};

export type Lookup = SelectionTarget & {
  term: string;
  entries: DictionaryEntry[];
};
