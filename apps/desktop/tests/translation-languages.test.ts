import assert from "node:assert/strict";
import test from "node:test";

import {
  canonicalLanguageTag,
  localizedLanguageName,
  supportsCustomTranslationLanguage,
  translationLanguageCodesForProfile,
} from "../src/translation-languages.ts";
import type { ProviderCapabilities } from "../src/types.ts";

function profileCapabilities(
  supportedLanguages: string[],
  supportsCustom: boolean,
): { capabilities: ProviderCapabilities } {
  return {
    capabilities: {
      supports_streaming: false,
      supports_model_listing: false,
      requires_api_key: true,
      is_local: false,
      supports_context: false,
      supports_translation: true,
      supports_asr: false,
      supports_text_generation: false,
      supports_custom_translation_language: supportsCustom,
      supported_languages: supportedLanguages,
    },
  };
}

test("translation language tags are canonicalized conservatively", () => {
  assert.equal(canonicalLanguageTag(" pt-br "), "pt-BR");
  assert.equal(canonicalLanguageTag("yue-hant"), "yue-Hant");
  assert.equal(canonicalLanguageTag("not a language"), null);
  assert.equal(canonicalLanguageTag("en-u-ca-gregory"), null);
});

test("language choices come from profile capability data", () => {
  const unrestricted = profileCapabilities([], true);
  const restricted = profileCapabilities(["en", "ja"], false);

  assert.equal(supportsCustomTranslationLanguage(unrestricted), true);
  assert.equal(supportsCustomTranslationLanguage(restricted), false);
  assert.equal(translationLanguageCodesForProfile(unrestricted).includes("hi"), true);
  assert.deepEqual(translationLanguageCodesForProfile(restricted), ["en", "ja"]);
});

test("language labels have a stable fallback", () => {
  assert.ok(localizedLanguageName("pt-BR", "en-US").length > 0);
  assert.equal(localizedLanguageName("tlh-Latn", "$invalid"), "tlh-Latn");
});
