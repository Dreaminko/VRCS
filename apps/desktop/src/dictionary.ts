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
