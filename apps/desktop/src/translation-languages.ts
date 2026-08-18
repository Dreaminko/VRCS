import type { ApiProfileView } from "./types";

export interface TranslationLanguage {
  code: string;
  englishName: string;
  nativeName: string;
}

export const TRANSLATION_LANGUAGES: readonly TranslationLanguage[] = [
  { code: "zh-Hans", englishName: "Chinese (Simplified)", nativeName: "简体中文" },
  { code: "zh-Hant", englishName: "Chinese (Traditional)", nativeName: "繁體中文" },
  { code: "yue-Hant", englishName: "Cantonese (Traditional)", nativeName: "粵語（繁體）" },
  { code: "en", englishName: "English", nativeName: "English" },
  { code: "ja", englishName: "Japanese", nativeName: "日本語" },
  { code: "ko", englishName: "Korean", nativeName: "한국어" },
  { code: "es", englishName: "Spanish", nativeName: "Español" },
  { code: "fr", englishName: "French", nativeName: "Français" },
  { code: "de", englishName: "German", nativeName: "Deutsch" },
  { code: "ru", englishName: "Russian", nativeName: "Русский" },
  { code: "ar", englishName: "Arabic", nativeName: "العربية" },
  { code: "bg", englishName: "Bulgarian", nativeName: "Български" },
  { code: "cs", englishName: "Czech", nativeName: "Čeština" },
  { code: "da", englishName: "Danish", nativeName: "Dansk" },
  { code: "el", englishName: "Greek", nativeName: "Ελληνικά" },
  { code: "he", englishName: "Hebrew", nativeName: "עברית" },
  { code: "hi", englishName: "Hindi", nativeName: "हिन्दी" },
  { code: "id", englishName: "Indonesian", nativeName: "Bahasa Indonesia" },
  { code: "it", englishName: "Italian", nativeName: "Italiano" },
  { code: "ms", englishName: "Malay", nativeName: "Bahasa Melayu" },
  { code: "nb", englishName: "Norwegian Bokmål", nativeName: "Norsk bokmål" },
  { code: "nl", englishName: "Dutch", nativeName: "Nederlands" },
  { code: "pl", englishName: "Polish", nativeName: "Polski" },
  { code: "pt-BR", englishName: "Portuguese (Brazil)", nativeName: "Português (Brasil)" },
  { code: "pt-PT", englishName: "Portuguese (Portugal)", nativeName: "Português (Portugal)" },
  { code: "ro", englishName: "Romanian", nativeName: "Română" },
  { code: "sv", englishName: "Swedish", nativeName: "Svenska" },
  { code: "th", englishName: "Thai", nativeName: "ไทย" },
  { code: "tr", englishName: "Turkish", nativeName: "Türkçe" },
  { code: "uk", englishName: "Ukrainian", nativeName: "Українська" },
  { code: "vi", englishName: "Vietnamese", nativeName: "Tiếng Việt" },
  { code: "fil", englishName: "Filipino", nativeName: "Filipino" },
  { code: "hu", englishName: "Hungarian", nativeName: "Magyar" },
  { code: "fi", englishName: "Finnish", nativeName: "Suomi" },
];

export const TRANSLATION_LANGUAGE_CODES = TRANSLATION_LANGUAGES.map(({ code }) => code);

export function translationLanguageCodesForProfile(
  profile?: Pick<ApiProfileView, "capabilities">,
): string[] {
  return profile?.capabilities.supported_languages.length
    ? [...profile.capabilities.supported_languages]
    : [...TRANSLATION_LANGUAGE_CODES];
}

export function supportsCustomTranslationLanguage(
  profile?: Pick<ApiProfileView, "capabilities">,
): boolean {
  return profile?.capabilities.supports_custom_translation_language ?? false;
}

export function canonicalLanguageTag(value: string): string | null {
  const input = value.trim();
  if (input.length < 2 || input.length > 35) return null;
  if (!/^[A-Za-z]{2,8}(?:-[A-Za-z0-9]{2,8})*$/.test(input)) return null;
  try {
    return Intl.getCanonicalLocales(input)[0] ?? null;
  } catch {
    return null;
  }
}

export function translationLanguage(code: string): TranslationLanguage | undefined {
  return TRANSLATION_LANGUAGES.find((language) => language.code === code);
}

export function localizedLanguageName(code: string, locale: string): string {
  const fallback = translationLanguage(code)?.englishName ?? code;
  try {
    return new Intl.DisplayNames([locale], {
      type: "language",
      languageDisplay: "dialect",
    }).of(code) ?? fallback;
  } catch {
    return fallback;
  }
}

export function languageSearchText(language: TranslationLanguage, locale: string): string {
  return [
    localizedLanguageName(language.code, locale),
    language.englishName,
    language.nativeName,
    language.code,
  ].join(" ");
}
