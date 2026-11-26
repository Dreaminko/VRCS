import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createInstance } from "i18next";

import { playerAudioDisclaimer } from "../src/capture/player-audio-disclaimer.ts";
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

test("every locale translates recognition runtime status values", async () => {
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
    for (const status of ["not_loaded", "loading", "ready", "error", "unknown"]) {
      const key = `settings.recognition.runtimeStatus.${status}`;
      const value = i18n.t(key);
      assert.ok(value && value !== key, `${locale}:${key} must have a translation`);
    }
  }
});

test("every locale provides standalone Windows and Linux network capture disclaimers", async () => {
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
    const t = i18n.t.bind(i18n);
    const windows = playerAudioDisclaimer(t, "windows");
    const linux = playerAudioDisclaimer(t, "linux");

    for (const [os, disclaimer] of [["windows", windows], ["linux", linux]] as const) {
      assert.ok(disclaimer.title.trim() && !disclaimer.title.startsWith("warnings.playerAudio."), `${locale}:${os}:title`);
      assert.ok(disclaimer.intro.trim() && !disclaimer.intro.startsWith("warnings.playerAudio."), `${locale}:${os}:intro`);
      assert.ok(disclaimer.confirm.trim() && !disclaimer.confirm.startsWith("warnings.playerAudio."), `${locale}:${os}:confirm`);
      const counting = disclaimer.readingStatus(2);
      const ready = disclaimer.readingStatus(0);
      assert.ok(counting.includes("2") && !counting.startsWith("warnings.playerAudio."), `${locale}:${os}:countdown`);
      assert.ok(ready.trim() && !ready.startsWith("warnings.playerAudio."), `${locale}:${os}:ready`);
      assert.equal(disclaimer.sections.length, 5);
      for (const section of disclaimer.sections) {
        assert.ok(section.title.trim() && !section.title.startsWith("warnings.playerAudio."), `${locale}:${os}:${section.key}:title`);
        for (const paragraph of section.paragraphs) {
          assert.ok(paragraph.trim() && !paragraph.startsWith("warnings.playerAudio.") && !paragraph.startsWith("settings."), `${locale}:${os}:${section.key}:paragraph`);
        }
      }
    }

    assert.notEqual(linux.sections[0].paragraphs[0], windows.sections[0].paragraphs[0]);
    assert.notEqual(linux.sections[0].paragraphs[1], windows.sections[0].paragraphs[1]);
    assert.notEqual(linux.sections[2].paragraphs[0], windows.sections[2].paragraphs[0]);
    const linuxText = [linux.intro, ...linux.sections.flatMap((section) => section.paragraphs)].join("\n");
    assert.match(linuxText, /polkit/i);
    assert.match(linuxText, /pkexec/i);
    assert.match(linuxText, /\/proc\/<pid>\/mem/);
    assert.match(linuxText, /CAP_SYS_PTRACE/);
    assert.doesNotMatch(linuxText, /windows|pagefile|分页文件|分頁檔|ページファイル/i);
  }
});
