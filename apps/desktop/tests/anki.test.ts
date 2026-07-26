import test from "node:test";
import assert from "node:assert/strict";
import { ankiButtonLabel } from "../src/anki.ts";
import { ankiDictionaryContent } from "../src/dictionary.ts";

test("Anki action labels remain recoverable after an error", () => {
  assert.equal(ankiButtonLabel("idle"), "添加到 Anki");
  assert.equal(ankiButtonLabel("adding"), "正在添加…");
  assert.equal(ankiButtonLabel("success"), "已添加到 Anki");
  assert.equal(ankiButtonLabel("error"), "重试添加");
});

test("collects every displayed dictionary definition for the Anki back field", () => {
  const content = ankiDictionaryContent([
    {
      term: "便利",
      reading: "べんり",
      language: "ja",
      dictionary: "日中词典",
      definition: "方便；有用",
    },
    {
      term: "便",
      reading: "べん",
      language: "ja",
      dictionary: "汉字词典",
      definition: "消息\n便利",
    },
  ]);

  assert.equal(
    content.definition,
    "【日中词典】\n1. 方便\n2. 有用\n\n【汉字词典 · 便】\n1. 消息\n2. 便利",
  );
  assert.equal(content.dictionary, "日中词典 · 汉字词典");
});
