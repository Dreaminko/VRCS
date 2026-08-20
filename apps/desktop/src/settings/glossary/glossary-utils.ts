import type { TFunction } from "i18next";

import type { GlossaryEntry, GlossarySource } from "../../types";

export const MAX_GLOSSARY_ENTRIES = 500;
export const MAX_GLOSSARY_NAME_LENGTH = 100;
export const MAX_GLOSSARY_TERM_LENGTH = 200;
export const MAX_GLOSSARY_URL_LENGTH = 2048;
export const GLOSSARY_CATEGORIES = ["person", "world", "game", "custom"] as const;

export interface GlossaryEntryDraft extends GlossaryEntry {
  rowId: string;
}

export interface LocalGlossaryDraft {
  id: string | null;
  name: string;
  entries: GlossaryEntryDraft[];
}

export interface SubscriptionGlossaryDraft {
  id: string | null;
  url: string;
  displayName: string;
}

export interface PublicGlossaryFile {
  version: 1;
  name?: string;
  entries: GlossaryEntry[];
}

export function emptyGlossaryEntry(): GlossaryEntry {
  return {
    source: "",
    target: "",
    category: "custom",
    case_sensitive: false,
  };
}

export function glossaryEntryDraft(entry: GlossaryEntry = emptyGlossaryEntry()): GlossaryEntryDraft {
  const suffix = typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return { ...entry, rowId: `entry-${suffix}` };
}

export function glossaryEntryValue(entry: GlossaryEntryDraft): GlossaryEntry {
  return {
    source: entry.source,
    target: entry.target,
    category: entry.category,
    case_sensitive: entry.case_sensitive,
  };
}

export function glossarySourceId(type: GlossarySource["type"]): string {
  const suffix = typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `${type}-${suffix}`;
}

export function characterCount(value: string): number {
  return Array.from(value).length;
}

function containsControl(value: string): boolean {
  return Array.from(value).some((character) => /[\u0000-\u001f\u007f-\u009f]/.test(character));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, allowed: readonly string[]): boolean {
  return Object.keys(value).every((key) => allowed.includes(key));
}

export function parsePublicGlossaryFile(value: unknown): PublicGlossaryFile | null {
  if (!isRecord(value) || !hasOnlyKeys(value, ["version", "name", "entries"])) return null;
  if (value.version !== 1 || !Array.isArray(value.entries)) return null;
  if (value.name !== undefined && typeof value.name !== "string") return null;

  const entries: GlossaryEntry[] = [];
  for (const item of value.entries) {
    if (!isRecord(item) || !hasOnlyKeys(item, ["source", "target", "category", "case_sensitive"])) {
      return null;
    }
    if (typeof item.source !== "string") return null;
    if (item.target !== undefined && item.target !== null && typeof item.target !== "string") return null;
    if (item.category !== undefined && !GLOSSARY_CATEGORIES.includes(item.category as GlossaryEntry["category"])) {
      return null;
    }
    if (item.case_sensitive !== undefined && typeof item.case_sensitive !== "boolean") return null;
    entries.push({
      source: item.source,
      target: item.target === undefined ? null : item.target as string | null,
      category: (item.category ?? "custom") as GlossaryEntry["category"],
      case_sensitive: item.case_sensitive ?? false,
    });
  }

  return {
    version: 1,
    ...(value.name === undefined ? {} : { name: value.name }),
    entries,
  };
}

export interface GlossaryEntryIssue {
  field: "source" | "target";
  message: string;
  row: number;
}

export function glossaryEntryIssues(entries: readonly GlossaryEntry[], t: TFunction): GlossaryEntryIssue[] {
  const issues: GlossaryEntryIssue[] = [];
  const sources = new Set<string>();
  for (const [index, entry] of entries.entries()) {
    const row = index + 1;
    const source = entry.source.trim();
    if (!source) {
      issues.push({ field: "source", message: t("settings.glossary.glossaryValidation.sourceRequired", { row }), row });
    } else if (containsControl(entry.source)) {
      issues.push({ field: "source", message: t("settings.glossary.glossaryValidation.sourceSingleLine", { row }), row });
    } else if (characterCount(source) > MAX_GLOSSARY_TERM_LENGTH) {
      issues.push({ field: "source", message: t("settings.glossary.glossaryValidation.sourceTooLong", { row }), row });
    }

    if (entry.target !== null && containsControl(entry.target)) {
      issues.push({ field: "target", message: t("settings.glossary.glossaryValidation.targetSingleLine", { row }), row });
    } else if (entry.target !== null && characterCount(entry.target) > MAX_GLOSSARY_TERM_LENGTH) {
      issues.push({ field: "target", message: t("settings.glossary.glossaryValidation.targetTooLong", { row }), row });
    }

    if (!source) continue;
    const duplicateKey = `${entry.case_sensitive ? source : source.toLowerCase()}\0${entry.case_sensitive}`;
    if (sources.has(duplicateKey)) {
      issues.push({ field: "source", message: t("settings.glossary.glossaryValidation.duplicateSource", { row, source }), row });
    }
    sources.add(duplicateKey);
  }
  return issues;
}

export function validateEntries(entries: GlossaryEntry[], t: TFunction): string {
  if (entries.length > MAX_GLOSSARY_ENTRIES) {
    return t("settings.glossary.glossaryValidation.tooMany", { count: MAX_GLOSSARY_ENTRIES });
  }
  return glossaryEntryIssues(entries, t)[0]?.message ?? "";
}

export function validateLocalDraft(draft: LocalGlossaryDraft, t: TFunction): string {
  const name = draft.name.trim();
  if (!name) return t("settings.glossary.glossaryValidation.nameRequired");
  if (characterCount(name) > MAX_GLOSSARY_NAME_LENGTH) {
    return t("settings.glossary.glossaryValidation.nameTooLong");
  }
  return validateEntries(draft.entries, t);
}

function isLoopbackHostname(hostname: string): boolean {
  const normalized = hostname.toLowerCase();
  if (normalized === "localhost") return true;
  if (normalized === "[::1]" || normalized === "::1") return true;
  const octets = normalized.split(".");
  return octets.length === 4
    && octets[0] === "127"
    && octets.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255);
}

export function validateSubscriptionDraft(draft: SubscriptionGlossaryDraft, t: TFunction): string {
  const url = draft.url.trim();
  if (!url) return t("settings.glossary.glossaryValidation.urlRequired");
  if (url.length > MAX_GLOSSARY_URL_LENGTH) {
    return t("settings.glossary.glossaryValidation.urlInvalid");
  }
  try {
    const parsed = new URL(url);
    if (!parsed.hostname || parsed.username || parsed.password || parsed.hash) {
      return t("settings.glossary.glossaryValidation.urlInvalid");
    }
    if (parsed.protocol !== "https:" && !(parsed.protocol === "http:" && isLoopbackHostname(parsed.hostname))) {
      return t("settings.glossary.glossaryValidation.urlInvalid");
    }
  } catch {
    return t("settings.glossary.glossaryValidation.urlInvalid");
  }
  if (characterCount(draft.displayName.trim()) > MAX_GLOSSARY_NAME_LENGTH) {
    return t("settings.glossary.glossaryValidation.displayNameTooLong");
  }
  return "";
}

export function exportFileName(name: string): string {
  const safeName = name.trim().replace(/[<>:"/\\|?*\u0000-\u001f]/g, "-").replace(/[. ]+$/g, "");
  return `${safeName || "glossary"}.json`;
}
