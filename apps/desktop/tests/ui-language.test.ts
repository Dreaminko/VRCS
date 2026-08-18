import assert from "node:assert/strict";
import test from "node:test";

import {
  isUiLanguagePreference,
  resolveUiLocale,
} from "../src/app/ui-language.ts";

const supportedLocales = ["zh-CN", "ja-JP", "en-US"];

test("resolves supported system language variants", () => {
  assert.equal(resolveUiLocale("system", supportedLocales, ["ja-JP", "en-US"]), "ja-JP");
  assert.equal(resolveUiLocale("system", supportedLocales, ["zh-Hant-TW"]), "zh-CN");
  assert.equal(resolveUiLocale("system", supportedLocales, ["en-GB"]), "en-US");
});

test("falls back to English and honors an explicit preference", () => {
  assert.equal(resolveUiLocale("system", supportedLocales, ["fr-FR"]), "en-US");
  assert.equal(resolveUiLocale("zh-CN", supportedLocales, ["ja-JP"]), "zh-CN");
});

test("accepts only supported language preferences", () => {
  assert.equal(isUiLanguagePreference("system", supportedLocales), true);
  assert.equal(isUiLanguagePreference("ja-JP", supportedLocales), true);
  assert.equal(isUiLanguagePreference("fr-FR", supportedLocales), false);
  assert.equal(isUiLanguagePreference(null, supportedLocales), false);
});
