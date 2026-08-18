import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { contentLanguageTag } from "../src/app/ui-language.ts";

const baseCss = readFileSync(new URL("../src/styles/base.css", import.meta.url), "utf8");

test("Simplified Chinese uses an SC font before Japanese CJK fonts", () => {
  const stack = baseCss.match(/--font-ui-zh-hans:\s*([^;]+);/)?.[1];
  assert.ok(stack, "the Simplified Chinese font stack should be defined");
  assert.ok(stack.indexOf('"Microsoft YaHei UI"') >= 0);
  assert.ok(stack.indexOf('"Yu Gothic UI"') === -1);
  assert.match(baseCss, /:lang\(zh-CN\)[\s\S]*?font-family:\s*var\(--font-ui-zh-hans\)/);
});

test("bare recognition language codes receive script-appropriate language tags", () => {
  assert.equal(contentLanguageTag("zh"), "zh-CN");
  assert.equal(contentLanguageTag("ja"), "ja-JP");
  assert.equal(contentLanguageTag("ko"), "ko-KR");
  assert.equal(contentLanguageTag("zh_Hans"), "zh-Hans");
  assert.equal(contentLanguageTag("auto"), undefined);
  assert.equal(contentLanguageTag(null), undefined);
});
