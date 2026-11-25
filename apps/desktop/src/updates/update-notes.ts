export const KNOWN_UPDATE_ERROR_CODES = new Set([
  "unavailable",
  "busy",
  "no_pending",
  "stable_check_failed",
  "nightly_check_failed",
  "both_checks_failed",
  "preference_save_failed",
  "timeout",
  "download_failed",
  "signature_invalid",
  "install_failed",
  "failed",
]);

export function resolveUpdateErrorKey(code: string | null | undefined): string {
  return code && KNOWN_UPDATE_ERROR_CODES.has(code) ? code : "failed";
}

/**
 * Strips automated CI platform installation appendices and Nightly footers
 * so the in-app viewer focuses on user-facing release notes.
 */
export function stripPlatformNotesSection(markdown: string): string {
  if (!markdown) return "";
  return markdown
    .replace(/\r\n/g, "\n")
    .replace(/<!--\s*vrcs-platform-notes-start\s*-->[\s\S]*?<!--\s*vrcs-platform-notes-end\s*-->/g, "")
    .replace(/<details>\s*<summary>\s*Platform(?:\s|&amp;|&)+Installation Notes\s*<\/summary>[\s\S]*?<\/details>/gi, "")
    .replace(/\n---\n+\s*Build date:\s*\d{4}-\d{2}-\d{2}[\s\S]*$/i, "")
    .trim();
}

function cleanInlineMarkdown(text: string): string {
  return text
    .replace(/!\[([^\]]*)\]\([^)]+\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/__([^_]+)__/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .replace(/_([^_]+)_/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Extracts up to `maxItems` concise highlights from markdown release notes
 * for the lightweight update toast and settings card.
 */
export function extractNotesPreview(
  markdown: string | null | undefined,
  maxItems = 3,
): string[] {
  if (!markdown || maxItems <= 0) return [];
  const cleaned = stripPlatformNotesSection(markdown);
  if (!cleaned) return [];

  const lines = cleaned.split("\n");
  const bullets: string[] = [];
  const prose: string[] = [];

  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#") || line === "---" || line.startsWith("<!--")) continue;
    if (/^\[Full Changelog\]\(/i.test(line)) continue;

    const bulletMatch = /^(?:[-*+]|\d+\.)\s+(.+)$/.exec(line);
    if (bulletMatch) {
      const item = cleanInlineMarkdown(bulletMatch[1]);
      if (item && !/^\.{3}and \d+ more change/i.test(item)) {
        bullets.push(item);
      }
      continue;
    }

    if (!line.startsWith("<")) {
      const text = cleanInlineMarkdown(line);
      if (text) prose.push(text);
    }
  }

  const source = bullets.length > 0 ? bullets : prose;
  const unique: string[] = [];
  for (const item of source) {
    if (!unique.includes(item)) {
      unique.push(item);
      if (unique.length >= maxItems) break;
    }
  }
  return unique;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatDownloadSpeed(bytesPerSec: number | null | undefined): string | null {
  if (!bytesPerSec || !Number.isFinite(bytesPerSec) || bytesPerSec <= 0) return null;
  return `${formatBytes(bytesPerSec)}/s`;
}

export function formatPublishDate(iso: string | null | undefined, locale: string): string | null {
  if (!iso?.trim()) return null;
  const timestamp = Date.parse(iso);
  if (!Number.isFinite(timestamp)) return null;
  try {
    return new Intl.DateTimeFormat(locale || "en-US", { dateStyle: "medium" }).format(new Date(timestamp));
  } catch {
    return new Intl.DateTimeFormat("en-US", { dateStyle: "medium" }).format(new Date(timestamp));
  }
}

export function formatLastCheckedTime(timestamp: number | null | undefined, locale: string): string | null {
  if (!timestamp || !Number.isFinite(timestamp) || timestamp <= 0) return null;
  try {
    return new Intl.DateTimeFormat(locale || "en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(timestamp));
  } catch {
    return new Intl.DateTimeFormat("en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(timestamp));
  }
}

export function isNightlyVersion(version: string | null | undefined): boolean {
  return typeof version === "string" && /-nightly\.\d{8}$/.test(version.trim());
}
