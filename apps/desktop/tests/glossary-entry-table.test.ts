import assert from "node:assert/strict";
import test from "node:test";

import {
  applyGlossaryEntryBulkAction,
  visibleGlossaryEntries,
} from "../src/settings/glossary/glossary-entry-table.ts";
import {
  glossaryEntryDraft,
  glossaryEntryValue,
} from "../src/settings/glossary/glossary-utils.ts";

function entry(source: string, target: string | null, category: "person" | "world" | "game" | "custom" = "custom") {
  return glossaryEntryDraft({ source, target, category, case_sensitive: false });
}

test("filters and sorts glossary rows without changing their stored positions", () => {
  const entries = [
    entry("VRChat", "VRChat"),
    entry("Black Cat", "黑猫酒吧", "world"),
    entry("Fallback", null),
  ];

  assert.deepEqual(
    visibleGlossaryEntries(entries, "cat", "all", "all", "source", "asc")
      .map(({ entry: item, index }) => [item.source, index]),
    [["Black Cat", 1]],
  );
  assert.deepEqual(
    visibleGlossaryEntries(entries, "", "all", "original", "position", "asc")
      .map(({ entry: item }) => item.source),
    ["Fallback"],
  );
});

test("applies a bulk action only to selected draft rows", () => {
  const entries = [entry("VRChat", "VRChat"), entry("Black Cat", "黑猫酒吧")];
  const updated = applyGlossaryEntryBulkAction(
    entries,
    new Set([entries[1].rowId]),
    "category:world",
  );

  assert.equal(updated[0], entries[0]);
  assert.equal(updated[1].category, "world");
  assert.equal(updated[1].rowId, entries[1].rowId);
});

test("removes transient row ids from persisted glossary values", () => {
  const draft = entry("VRChat", null, "game");
  assert.deepEqual(glossaryEntryValue(draft), {
    source: "VRChat",
    target: null,
    category: "game",
    case_sensitive: false,
  });
});
