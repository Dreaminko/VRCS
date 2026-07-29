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

const modules = import.meta.glob("./locales/*.json", {
  eager: true,
  import: "default",
}) as Record<string, LocaleResource>;

export const localeCatalog = Object.entries(modules)
  .map(([path, resource]) => {
    const filename = path.split("/").pop()?.replace(/\.json$/, "");
    if (!filename || filename !== resource._meta.locale) {
      throw new Error(`Locale filename and metadata differ: ${path}`);
    }
    return resource;
  })
  .sort((left, right) => left._meta.locale.localeCompare(right._meta.locale));

if (!localeCatalog.some((resource) => resource._meta.locale === "en-US")) {
  throw new Error("The en-US fallback locale is required");
}

export const supportedUiLocales = localeCatalog.map(
  (resource) => resource._meta.locale,
);

export const localeResources = Object.fromEntries(
  localeCatalog.map((resource) => [
    resource._meta.locale,
    { translation: resource.translation },
  ]),
);
