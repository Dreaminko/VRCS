import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { validateLocalization } from "../../../scripts/check-i18n.mjs";

type JsonObject = Record<string, unknown>;

const localeDirectory = new URL("../src/i18n/locales/", import.meta.url);
const localeNames = readdirSync(localeDirectory)
  .filter((filename) => filename.endsWith(".json"))
  .map((filename) => filename.replace(/\.json$/, ""))
  .sort();

function loadLocale(name: string): JsonObject {
  return JSON.parse(
    readFileSync(new URL(`../src/i18n/locales/${name}.json`, import.meta.url), "utf8"),
  ) as JsonObject;
}

function leaves(value: unknown, prefix = ""): Map<string, string> {
  const result = new Map<string, string>();
  if (typeof value === "string") {
    result.set(prefix, value);
    return result;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return result;
  for (const [key, child] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    for (const [leaf, text] of leaves(child, path)) result.set(leaf, text);
  }
  return result;
}

function placeholders(value: string): string[] {
  return [...value.matchAll(/\{\{\s*([^},\s]+)[^}]*\}\}/g)]
    .map((match) => match[1])
    .sort();
}

function sourceFiles(directory: URL): URL[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const child = new URL(entry.name + (entry.isDirectory() ? "/" : ""), directory);
    if (entry.isDirectory()) return sourceFiles(child);
    return /\.(?:ts|tsx)$/.test(entry.name) ? [child] : [];
  });
}

test("locale resources have identical keys and interpolation variables", () => {
  const resources = localeNames.map((name) => {
    const resource = loadLocale(name);
    assert.equal((resource._meta as JsonObject).status, "complete");
    return [name, leaves(resource.translation)] as const;
  });
  const referenceResource = resources.find(([name]) => name === "en-US");
  assert.ok(referenceResource);
  const [referenceName, reference] = referenceResource;

  for (const [name, resource] of resources.filter(([name]) => name !== referenceName)) {
    assert.deepEqual(
      [...resource.keys()].sort(),
      [...reference.keys()].sort(),
      `${name} keys differ from ${referenceName}`,
    );
    for (const [key, referenceValue] of reference) {
      const value = resource.get(key);
      assert.ok(value?.trim(), `${name}:${key} must not be empty`);
      assert.deepEqual(
        placeholders(value!),
        placeholders(referenceValue),
        `${name}:${key} interpolation variables differ`,
      );
    }
  }
});

test("repository localization validator accepts every locale", () => {
  assert.deepEqual(
    validateLocalization(fileURLToPath(new URL("../../..", import.meta.url))),
    [],
  );
});

test("every statically referenced translation key exists", () => {
  const english = leaves(loadLocale("en-US").translation);
  for (const sourceFile of sourceFiles(new URL("../src/", import.meta.url))) {
    const source = readFileSync(sourceFile, "utf8");
    for (const match of source.matchAll(/\bt\("([^"]+)"/g)) {
      assert.ok(
        english.has(match[1]),
        `${fileURLToPath(sourceFile)} references missing key ${match[1]}`,
      );
    }
  }
});
