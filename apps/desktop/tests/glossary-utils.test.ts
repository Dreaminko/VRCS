import assert from "node:assert/strict";
import test from "node:test";
import type { TFunction } from "i18next";

import type { GlossaryEntry } from "../src/types.ts";
import {
  parsePublicGlossaryFile,
  validateEntries,
  validateSubscriptionDraft,
} from "../src/settings/translation/glossary-utils.ts";

const t = ((key: string) => key) as TFunction;

function entry(source: string, caseSensitive = false): GlossaryEntry {
  return {
    source,
    target: null,
    category: "custom",
    case_sensitive: caseSensitive,
  };
}

test("parses the public glossary format and applies entry defaults", () => {
  assert.deepEqual(parsePublicGlossaryFile({
    version: 1,
    name: "Shared glossary",
    entries: [{ source: "VRChat" }],
  }), {
    version: 1,
    name: "Shared glossary",
    entries: [{
      source: "VRChat",
      target: null,
      category: "custom",
      case_sensitive: false,
    }],
  });
});

test("rejects unknown fields in public glossary files", () => {
  assert.equal(parsePublicGlossaryFile({
    version: 1,
    entries: [],
    unknown: true,
  }), null);
  assert.equal(parsePublicGlossaryFile({
    version: 1,
    entries: [{ source: "VRChat", unknown: true }],
  }), null);
});

test("matches Core duplicate rules for case-sensitive glossary entries", () => {
  assert.equal(
    validateEntries([entry("VRChat"), entry("vrchat")], t),
    "settings.translation.glossaryValidation.duplicateSource",
  );
  assert.equal(validateEntries([entry("VRChat", true), entry("vrchat", true)], t), "");
  assert.equal(validateEntries([entry("VRChat", true), entry("VRChat")], t), "");
});

test("allows HTTPS and loopback HTTP subscription URLs only", () => {
  assert.equal(validateSubscriptionDraft({
    id: null,
    url: "https://example.com/glossary.json",
    displayName: "",
  }, t), "");
  assert.equal(validateSubscriptionDraft({
    id: null,
    url: "http://127.0.0.1:8080/glossary.json",
    displayName: "",
  }, t), "");
  assert.equal(validateSubscriptionDraft({
    id: null,
    url: "http://example.com/glossary.json",
    displayName: "",
  }, t), "settings.translation.glossaryValidation.urlInvalid");
});
