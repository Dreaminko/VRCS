import type { DictionaryEntry } from "./types";
import type { LookupAnchor } from "./popover-placement";

export type Page = "live" | "history" | "settings";

export type Lookup = {
  term: string;
  context: string;
  entries: DictionaryEntry[];
  anchor: LookupAnchor;
  range?: Range;
};
