import assert from "node:assert/strict";
import test from "node:test";
import { definitionGlosses, groupDictionaryEntries } from "../src/dictionary/dictionary.ts";

test("groups repeated headword entries and removes duplicate glosses", () => {
  const grouped = groupDictionaryEntries([
    { term: "便利", reading: "べんり", language: "ja", dictionary: "测试词典", definition: "方便\n有用" },
    { term: "便利", reading: "べんり", language: "ja", dictionary: "测试词典", definition: "有用；省事" },
  ]);

  assert.equal(grouped.length, 1);
  assert.deepEqual(definitionGlosses(grouped[0].definition), ["方便", "有用", "省事"]);
});

test("keeps different headwords and dictionary sources separate", () => {
  const grouped = groupDictionaryEntries([
    { term: "あ", reading: "", language: "ja", dictionary: "词典 A", definition: "感叹词" },
    { term: "亜", reading: "あ", language: "ja", dictionary: "词典 A", definition: "亚、次等" },
    { term: "あ", reading: "", language: "ja", dictionary: "词典 B", definition: "感叹词" },
  ]);

  assert.equal(grouped.length, 3);
});

test("collapses spelling variants with the same reading and definition", () => {
  const grouped = groupDictionaryEntries([
    { term: "話し相手", reading: "はなしあいて", language: "ja", dictionary: "测试词典", definition: "交谈的对象" },
    { term: "話しあいて", reading: "はなしあいて", language: "ja", dictionary: "测试词典", definition: "交谈的对象" },
  ]);

  assert.deepEqual(grouped.map((entry) => entry.term), ["話し相手"]);
});
