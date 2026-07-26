import type { DictionaryEntry } from "./types";

export function definitionGlosses(definition: string): string[] {
  const normalized = definition.replace(/([①②③④⑤⑥⑦⑧⑨⑩])/g, "\n$1");
  const parts = normalized
    .split(/\n+|[；;]/)
    .map((part) => part.trim())
    .filter(Boolean);
  return parts.length ? parts : [definition];
}

export function groupDictionaryEntries(entries: DictionaryEntry[]): DictionaryEntry[] {
  const groups = new Map<string, { entry: DictionaryEntry; glosses: string[]; seen: Set<string> }>();
  const variants = new Set<string>();

  for (const entry of entries) {
    const glosses = definitionGlosses(entry.definition);
    const variantKey = JSON.stringify([
      entry.dictionary ?? "",
      entry.reading ?? "",
      entry.language,
      glosses,
    ]);
    if (variants.has(variantKey)) continue;
    variants.add(variantKey);

    const key = JSON.stringify([entry.dictionary ?? "", entry.term, entry.reading ?? "", entry.language]);
    let group = groups.get(key);
    if (!group) {
      group = { entry, glosses: [], seen: new Set() };
      groups.set(key, group);
    }
    for (const gloss of glosses) {
      if (group.seen.has(gloss)) continue;
      group.seen.add(gloss);
      group.glosses.push(gloss);
    }
  }

  return [...groups.values()].map(({ entry, glosses }) => ({
    ...entry,
    definition: glosses.join("\n"),
  }));
}

export interface AnkiDictionaryContent {
  definition: string;
  dictionary: string | null;
}

export function ankiDictionaryContent(entries: DictionaryEntry[]): AnkiDictionaryContent {
  const sources = new Set<string>();
  const primaryTerm = entries[0]?.term;
  const sections = entries.map((entry) => {
    const source = entry.dictionary || "内置词典";
    sources.add(source);
    const variant = entry.term !== primaryTerm ? ` · ${entry.term}` : "";
    const glosses = definitionGlosses(entry.definition)
      .map((gloss, index) => `${index + 1}. ${gloss}`)
      .join("\n");
    return `【${source}${variant}】\n${glosses}`;
  });
  return {
    definition: sections.join("\n\n"),
    dictionary: sources.size ? [...sources].join(" · ") : null,
  };
}
