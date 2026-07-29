import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

type JsonObject = Record<string, unknown>;

const localeNames = ["en-US", "ja-JP", "zh-CN"] as const;

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

test("locale resources have identical keys and interpolation variables", () => {
  const resources = localeNames.map((name) => {
    const resource = loadLocale(name);
    assert.equal((resource._meta as JsonObject).status, "complete");
    return [name, leaves(resource.translation)] as const;
  });
  const [referenceName, reference] = resources[0];

  for (const [name, resource] of resources.slice(1)) {
    assert.deepEqual(
      [...resource.keys()].sort(),
      [...reference.keys()].sort(),
      `${name} keys differ from ${referenceName}`,
    );
    for (const [key, referenceValue] of reference) {
      const value = resource.get(key);
      assert.ok(value?.trim(), `${name}:${key} must not be empty`);
      assert.deepEqual(
        placeholders(value),
        placeholders(referenceValue),
        `${name}:${key} interpolation variables differ`,
      );
    }
  }
});

test("every statically referenced translation key exists", () => {
  const english = leaves(loadLocale("en-US").translation);
  const sourceFiles = [
    "../src/App.tsx",
    "../src/i18n/index.ts",
  ];
  for (const sourceFile of sourceFiles) {
    const source = readFileSync(new URL(sourceFile, import.meta.url), "utf8");
    for (const match of source.matchAll(/\bt\("([^"]+)"/g)) {
      assert.ok(english.has(match[1]), `${sourceFile} references missing key ${match[1]}`);
    }
  }
});
