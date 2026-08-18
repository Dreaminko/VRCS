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

export type Lookup = {
  term: string;
  context: string;
  entries: DictionaryEntry[];
  origin?: LookupOrigin;
  anchor: LookupAnchor;
  range?: Range;
};
