import assert from "node:assert/strict";
import test from "node:test";

import {
  canonicalLanguageTag,
  localizedLanguageName,
  supportsCustomTranslationLanguage,
  translationLanguageCodesForProvider,
} from "../src/translation-languages.ts";

test("translation language tags are canonicalized conservatively", () => {
  assert.equal(canonicalLanguageTag(" pt-br "), "pt-BR");
  assert.equal(canonicalLanguageTag("yue-hant"), "yue-Hant");
  assert.equal(canonicalLanguageTag("not a language"), null);
  assert.equal(canonicalLanguageTag("en-u-ca-gregory"), null);
});

test("provider language choices distinguish LLM and DeepL capabilities", () => {
  assert.equal(supportsCustomTranslationLanguage("openai"), true);
  assert.equal(supportsCustomTranslationLanguage("deepl"), false);
  assert.equal(translationLanguageCodesForProvider("openai").includes("hi"), true);
  assert.equal(translationLanguageCodesForProvider("deepl").includes("hi"), false);
});

test("language labels have a stable fallback", () => {
  assert.ok(localizedLanguageName("pt-BR", "en-US").length > 0);
  assert.equal(localizedLanguageName("tlh-Latn", "$invalid"), "tlh-Latn");
});
