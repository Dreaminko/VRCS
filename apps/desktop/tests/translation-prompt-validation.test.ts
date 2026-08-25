import assert from "node:assert/strict";
import test from "node:test";

import { translationPromptVariableError } from "../src/settings/translation/translation-prompt-validation.ts";

test("translation prompt validation accepts every supported variable", () => {
  assert.equal(
    translationPromptVariableError(
      "Translate from {source_language} to {target_language}.{glossary}{context}",
    ),
    null,
  );
});

test("translation prompt validation identifies an unsupported text variable", () => {
  assert.equal(
    translationPromptVariableError("<source_text>{text}</source_text>"),
    "Unsupported translation prompt variable: {text}",
  );
});

test("translation prompt validation identifies malformed variable braces", () => {
  assert.equal(
    translationPromptVariableError("Translate {{target_language}}"),
    "Translation system prompt contains a nested variable",
  );
  assert.equal(
    translationPromptVariableError("Translate target_language}"),
    "Translation system prompt contains an unmatched closing brace",
  );
  assert.equal(
    translationPromptVariableError("Translate {target_language"),
    "Translation system prompt contains an unclosed variable",
  );
});
