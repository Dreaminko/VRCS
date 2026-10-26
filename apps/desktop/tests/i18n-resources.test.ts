import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createInstance } from "i18next";

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

function sourceFiles(directory: URL): URL[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const child = new URL(entry.name + (entry.isDirectory() ? "/" : ""), directory);
    if (entry.isDirectory()) return sourceFiles(child);
    return /\.(?:ts|tsx)$/.test(entry.name) ? [child] : [];
  });
}

test("repository localization validator accepts every locale", () => {
  assert.deepEqual(
    validateLocalization(fileURLToPath(new URL("../../..", import.meta.url))),
    [],
  );
});

test("desktop OCR and vision use generic screen capture errors in every locale", async () => {
  const i18n = createInstance();
  await i18n.init({
    resources: Object.fromEntries(localeNames.map((name) => [name, {
      translation: loadLocale(name).translation as JsonObject,
    }])),
    lng: "en-US",
    fallbackLng: "en-US",
    interpolation: { escapeValue: false },
  });

  for (const locale of localeNames) {
    await i18n.changeLanguage(locale);
    const resource = leaves(loadLocale(locale).translation);
    for (const window of ["ocrWindow", "visionWindow"]) {
      for (const code of ["capture_failed", "unavailable", "capture_timeout"]) {
        const key = `${window}.errors.${code}`;
        assert.ok(resource.get(key)?.trim(), `${locale}:${key} must not be empty`);
        assert.equal(i18n.t(key), resource.get(key), `${locale}:${key} must use the current locale`);
      }
      for (const code of ["not_game", "game_not_found", "minimized"]) {
        assert.equal(resource.has(`${window}.errors.${code}`), false, `${locale}:${window} retains ${code}`);
      }
    }
  }
});

test("VR OCR progress labels follow the UI language and preserve runtime placeholders", async () => {
  const i18n = createInstance();
  await i18n.init({
    resources: Object.fromEntries(localeNames.map((name) => [name, {
      translation: loadLocale(name).translation as JsonObject,
    }])),
    lng: "en-US",
    fallbackLng: "en-US",
    interpolation: { escapeValue: false },
  });

  for (const locale of localeNames) {
    await i18n.changeLanguage(locale);
    const labels = i18n.t("settings.vrOcr.progress", { returnObjects: true }) as Record<string, string>;
    const expected = ((loadLocale(locale).translation as JsonObject).settings as JsonObject).vrOcr as JsonObject;
    assert.deepEqual(labels, expected.progress, `${locale} progress labels differ`);
    assert.ok(labels.elapsed.includes("{{seconds}}"));
    assert.ok(labels.tasks.includes("{{completed}}"));
    assert.ok(labels.tasks.includes("{{total}}"));
    assert.ok(labels.failed.includes("{{failed}}"));
    assert.notEqual(labels.recognizing, "settings.vrOcr.progress.recognizing");
  }
});

test("every statically referenced translation key exists", () => {
  const english = leaves(loadLocale("en-US").translation);
  for (const sourceFile of sourceFiles(new URL("../src/", import.meta.url))) {
    const source = readFileSync(sourceFile, "utf8");
    for (const match of source.matchAll(/\bt\("([^"]+)"([^)]*)\)/g)) {
      const objectResource = match[2].includes("returnObjects: true")
        && [...english.keys()].some((key) => key.startsWith(`${match[1]}.`));
      assert.ok(
        english.has(match[1]) || objectResource,
        `${fileURLToPath(sourceFile)} references missing key ${match[1]}`,
      );
    }
  }
});
