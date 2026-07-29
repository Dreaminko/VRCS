import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const localeDirectory = join(repositoryRoot, "apps", "desktop", "src", "i18n", "locales");

function usage() {
  console.log('Usage: npm run i18n:new -- <locale> "<native name>" [ltr|rtl]');
  console.log('Example: npm run i18n:new -- fr-FR "Français"');
}

function emptyTranslations(value) {
  if (typeof value === "string") return "";
  return Object.fromEntries(
    Object.entries(value).map(([key, child]) => [key, emptyTranslations(child)]),
  );
}

const [localeInput, nativeName, direction = "ltr"] = process.argv.slice(2);
if (!localeInput || !nativeName || !["ltr", "rtl"].includes(direction)) {
  usage();
  process.exitCode = 1;
} else {
  let locale;
  try {
    locale = Intl.getCanonicalLocales(localeInput)[0];
  } catch {
    console.error(`${localeInput} is not a valid BCP 47 locale.`);
    process.exitCode = 1;
  }

  if (locale) {
    const target = join(localeDirectory, `${locale}.json`);
    if (existsSync(target)) {
      console.error(`${locale}.json already exists.`);
      process.exitCode = 1;
    } else {
      const reference = JSON.parse(
        readFileSync(join(localeDirectory, "en-US.json"), "utf8"),
      );
      const resource = {
        _meta: {
          locale,
          name: nativeName,
          direction,
          status: "complete",
        },
        translation: emptyTranslations(reference.translation),
      };
      writeFileSync(target, `${JSON.stringify(resource, null, 2)}\n`, "utf8");
      console.log(`Created ${target}`);
      console.log("Translate every empty value, then run npm run check:i18n.");
    }
  }
}
