import assert from "node:assert/strict";
import test from "node:test";

import {
  isUiLanguagePreference,
  resolveUiLocale,
} from "../src/ui-language.ts";

test("resolves supported system language variants", () => {
  assert.equal(resolveUiLocale("system", ["ja-JP", "en-US"]), "ja-JP");
  assert.equal(resolveUiLocale("system", ["zh-Hant-TW"]), "zh-CN");
  assert.equal(resolveUiLocale("system", ["en-GB"]), "en-US");
});

test("falls back to English and honors an explicit preference", () => {
  assert.equal(resolveUiLocale("system", ["fr-FR"]), "en-US");
  assert.equal(resolveUiLocale("zh-CN", ["ja-JP"]), "zh-CN");
});

test("accepts only supported language preferences", () => {
  assert.equal(isUiLanguagePreference("system"), true);
  assert.equal(isUiLanguagePreference("ja-JP"), true);
  assert.equal(isUiLanguagePreference("fr-FR"), false);
  assert.equal(isUiLanguagePreference(null), false);
});
