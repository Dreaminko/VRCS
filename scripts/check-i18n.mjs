import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const defaultRepositoryRoot = join(scriptDirectory, "..");
const requiredMetadata = ["locale", "name", "direction", "status"];
const sourceFilesWithTranslationKeys = [
  "apps/desktop/src/app/App.tsx",
  "apps/desktop/src/anki/anki.ts",
  "apps/desktop/src/i18n/index.ts",
  "apps/desktop/src/settings/settings-validation.ts",
];
const requiredRuntimeKeys = [
  "capture.pause",
  "capture.start",
  "capture.stop",
  "status.connection.connected",
  "status.connection.connecting",
  "status.connection.disconnected",
  "apiStatus.anki.connected",
  "apiStatus.anki.disabled",
  "apiStatus.anki.duplicate",
  "apiStatus.anki.incompatible_version",
  "apiStatus.anki.invalid_configuration",
  "apiStatus.anki.missing_deck",
  "apiStatus.anki.missing_field",
  "apiStatus.anki.missing_model",
  "apiStatus.anki.protocol_error",
  "apiStatus.anki.unavailable",
  "errors.anki.card_invalid",
  "errors.anki.disabled",
  "errors.anki.duplicate",
  "errors.anki.incompatible_version",
  "errors.anki.invalid_configuration",
  "errors.anki.missing_deck",
  "errors.anki.missing_field",
  "errors.anki.missing_model",
  "errors.anki.protocol_error",
  "errors.anki.unavailable",
  "errors.audio.device_unavailable",
  "errors.audio.unavailable",
  "errors.audio.vrchat_not_running",
  "errors.asr.model.delete_failed",
  "errors.asr.model.describe_failed",
  "errors.asr.model.download_start_failed",
  "errors.asr.model.in_use",
  "errors.asr.model.inspect_failed",
  "errors.asr.model.not_downloaded",
  "errors.asr.model.unsupported",
  "errors.auth.unauthorized",
  "errors.capture.already_running",
  "errors.capture.invalid_sample_rate",
  "errors.dictionary.delete_failed",
  "errors.dictionary.import_invalid",
  "errors.dictionary.import_task_failed",
  "errors.dictionary.invalid_query",
  "errors.dictionary.list_failed",
  "errors.dictionary.lookup_failed",
  "errors.dictionary.not_found",
  "errors.settings.asr_update_failed",
  "errors.settings.asr_update_task_failed",
  "errors.settings.capture_must_be_stopped",
  "errors.settings.invalid",
  "errors.settings.model_directory_migration_failed",
  "errors.settings.rollback_failed",
  "errors.subtitles.history_failed",
  "errors.subtitles.invalid_limit",
];

function readJson(path, errors) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    errors.push(`${path}: invalid JSON (${error.message})`);
    return null;
  }
}

function flattenStrings(value, prefix = "", output = new Map(), errors = [], file = "") {
  if (typeof value === "string") {
    output.set(prefix, value);
    return output;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    errors.push(`${file}:${prefix || "translation"} must be an object or string`);
    return output;
  }
  for (const [key, child] of Object.entries(value)) {
    flattenStrings(child, prefix ? `${prefix}.${key}` : key, output, errors, file);
  }
  return output;
}

function placeholders(value) {
  return [...value.matchAll(/\{\{\s*([^},\s]+)[^}]*\}\}/g)]
    .map((match) => match[1])
    .sort();
}

function arraysEqual(left, right) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function canonicalLocale(locale) {
  try {
    return Intl.getCanonicalLocales(locale)[0];
  } catch {
    return null;
  }
}

export function validateLocalization(repositoryRoot = defaultRepositoryRoot) {
  const errors = [];
  const localeDirectory = join(repositoryRoot, "apps", "desktop", "src", "i18n", "locales");
  const filenames = readdirSync(localeDirectory)
    .filter((filename) => filename.endsWith(".json"))
    .sort();
  const resources = [];
  const seenLocales = new Set();

  if (!filenames.length) errors.push("No locale files were found");

  for (const filename of filenames) {
    const path = join(localeDirectory, filename);
    const resource = readJson(path, errors);
    if (!resource) continue;
    const label = relative(repositoryRoot, path).replaceAll("\\", "/");
    const metadata = resource._meta;
    if (!metadata || typeof metadata !== "object" || Array.isArray(metadata)) {
      errors.push(`${label}: _meta must be an object`);
      continue;
    }
    for (const field of requiredMetadata) {
      if (typeof metadata[field] !== "string" || !metadata[field].trim()) {
        errors.push(`${label}: _meta.${field} must be a non-empty string`);
      }
    }
    const locale = metadata.locale;
    const filenameLocale = filename.replace(/\.json$/, "");
    const canonical = typeof locale === "string" ? canonicalLocale(locale) : null;
    if (!canonical) {
      errors.push(`${label}: _meta.locale is not a valid BCP 47 locale`);
    } else if (canonical !== locale) {
      errors.push(`${label}: use canonical locale ${canonical}`);
    }
    if (locale !== filenameLocale) {
      errors.push(`${label}: filename must match _meta.locale (${locale})`);
    }
    if (seenLocales.has(locale)) errors.push(`${label}: duplicate locale ${locale}`);
    seenLocales.add(locale);
    if (!["ltr", "rtl"].includes(metadata.direction)) {
      errors.push(`${label}: _meta.direction must be "ltr" or "rtl"`);
    }
    if (metadata.status !== "complete") {
      errors.push(`${label}: _meta.status must be "complete" before submission`);
    }
    if (!resource.translation || typeof resource.translation !== "object") {
      errors.push(`${label}: translation must be an object`);
      continue;
    }
    const strings = flattenStrings(resource.translation, "", new Map(), errors, label);
    resources.push({ label, locale, strings });
  }

  const reference = resources.find((resource) => resource.locale === "en-US");
  if (!reference) {
    errors.push("The required en-US reference locale is missing");
    return errors;
  }
  const referenceKeys = [...reference.strings.keys()].sort();

  for (const resource of resources) {
    const keys = [...resource.strings.keys()].sort();
    const missing = referenceKeys.filter((key) => !resource.strings.has(key));
    const extra = keys.filter((key) => !reference.strings.has(key));
    for (const key of missing) errors.push(`${resource.label}: missing translation key ${key}`);
    for (const key of extra) errors.push(`${resource.label}: unknown translation key ${key}`);
    for (const key of referenceKeys) {
      const value = resource.strings.get(key);
      if (value !== undefined && !value.trim()) {
        errors.push(`${resource.label}: ${key} must not be empty`);
      }
      if (
        value !== undefined
        && !arraysEqual(placeholders(value), placeholders(reference.strings.get(key)))
      ) {
        errors.push(`${resource.label}: ${key} has different interpolation variables`);
      }
    }
  }

  for (const sourceFile of sourceFilesWithTranslationKeys) {
    const path = join(repositoryRoot, ...sourceFile.split("/"));
    const source = readFileSync(path, "utf8");
    for (const match of source.matchAll(/\b(?:t|translate)\("([^"]+)"/g)) {
      if (!reference.strings.has(match[1])) {
        errors.push(`${sourceFile}: missing en-US translation key ${match[1]}`);
      }
    }
  }
  for (const key of requiredRuntimeKeys) {
    if (!reference.strings.has(key)) {
      errors.push(`Runtime references missing en-US translation key ${key}`);
    }
  }

  return errors;
}

function run() {
  const errors = validateLocalization();
  if (errors.length) {
    console.error(`Localization validation failed with ${errors.length} error(s):`);
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  const count = readdirSync(
    join(defaultRepositoryRoot, "apps", "desktop", "src", "i18n", "locales"),
  ).filter((filename) => filename.endsWith(".json")).length;
  console.log(`Localization validation passed for ${count} locale(s).`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) run();
