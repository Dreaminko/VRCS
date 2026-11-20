import localeMetadata from "./locales-meta.json";

export interface LocaleMetadata {
  locale: string;
  name: string;
  direction: "ltr" | "rtl";
  status: "complete";
}

export interface LocaleResource {
  _meta: LocaleMetadata;
  translation: Record<string, unknown>;
}

const loaders = import.meta.glob("./locales/*.json", {
  import: "default",
}) as Record<string, () => Promise<LocaleResource>>;

export const localeCatalog = (localeMetadata as readonly LocaleMetadata[]).map((_meta) => ({ _meta }));

export const supportedUiLocales = localeCatalog.map(
  (resource) => resource._meta.locale,
);

export function loadLocaleResource(locale: string): Promise<LocaleResource> {
  const loader = loaders[`./locales/${locale}.json`];
  if (!loader) throw new Error(`Unsupported locale: ${locale}`);
  return loader();
}
