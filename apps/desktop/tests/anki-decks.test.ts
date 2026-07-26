import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  ankiDeckAncestors,
  ankiDeckDisplayName,
  ankiDeckParent,
  buildAnkiDeckTree,
  visibleAnkiDeckNodes,
} from "../src/anki-decks.ts";

test("builds a selectable hierarchy from Anki deck separators", () => {
  const tree = buildAnkiDeckTree([
    "Default",
    "Language",
    "Language::Japanese::N5",
    "Language::Japanese::N4",
    "Language::English",
  ]);

  assert.deepEqual(tree.map((node) => node.name), ["Default", "Language"]);
  const language = tree[1];
  assert.equal(language.selectable, true);
  assert.deepEqual(language.children.map((node) => node.name), [
    "Language::English",
    "Language::Japanese",
  ]);
  assert.equal(language.children[1].selectable, false);
  assert.deepEqual(language.children[1].children.map((node) => node.label), ["N4", "N5"]);
});

test("only exposes descendants of expanded deck groups", () => {
  const tree = buildAnkiDeckTree([
    "Language",
    "Language::Japanese",
    "Language::Japanese::N5",
    "Language::English",
  ]);

  assert.deepEqual(
    visibleAnkiDeckNodes(tree, new Set()).map((node) => node.name),
    ["Language"],
  );
  assert.deepEqual(
    visibleAnkiDeckNodes(tree, new Set(["Language"])).map((node) => node.name),
    ["Language", "Language::English", "Language::Japanese"],
  );
  assert.deepEqual(
    visibleAnkiDeckNodes(tree, new Set(["Language", "Language::Japanese"])).map(
      (node) => node.name,
    ),
    ["Language", "Language::English", "Language::Japanese", "Language::Japanese::N5"],
  );
});

test("provides display paths and navigation relatives for nested decks", () => {
  assert.deepEqual(ankiDeckAncestors("Language::Japanese::N5"), [
    "Language",
    "Language::Japanese",
  ]);
  assert.equal(ankiDeckParent("Language::Japanese::N5"), "Language::Japanese");
  assert.equal(ankiDeckParent("Default"), null);
  assert.equal(ankiDeckDisplayName("Language::Japanese::N5"), "Language / Japanese / N5");
});

test("deck tree focus is not retriggered by visible list renders", () => {
  const source = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  const focusEffect = source.match(
    /useEffect\(\(\) => \{\s*if \(!open\) return;[\s\S]*?itemRefs\.current\.get\(activeName\)\?\.focus\(\)[\s\S]*?\}, \[([^\]]+)\]\);/,
  );

  assert.ok(focusEffect, "the deck focus effect must remain explicit and testable");
  assert.match(focusEffect[1], /\bactiveName\b/);
  assert.match(focusEffect[1], /\bopen\b/);
  assert.doesNotMatch(focusEffect[1], /\bvisibleNodes\b/);
});
