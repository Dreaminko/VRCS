function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Extracts the markdown body for `version` from a Keep-a-Changelog document.
 * Matches headings such as `## [0.4.0] - 2026-11-18` or `## 0.4.0`.
 */
export function extractChangelogSection(markdown, version) {
  if (typeof markdown !== "string" || !markdown.trim() || typeof version !== "string" || !version.trim()) {
    return null;
  }
  const normalized = markdown.replace(/\r\n/g, "\n");
  const lines = normalized.split("\n");
  const headingPattern = new RegExp(`^##\\s+\\[?v?${escapeRegExp(version.trim())}\\]?(?:\\s|$|[-–—])`);
  const startIndex = lines.findIndex((line) => headingPattern.test(line));
  if (startIndex < 0) return null;

  let endIndex = lines.length;
  for (let index = startIndex + 1; index < lines.length; index += 1) {
    if (/^##\s+/.test(lines[index])) {
      endIndex = index;
      break;
    }
  }
  const body = lines.slice(startIndex + 1, endIndex).join("\n").trim();
  return body || null;
}

const conventionalPattern = /^([a-zA-Z]+)(?:\(([^)]+)\))?!?:\s*(.+)$/;
const noisyTypes = new Set(["style", "test", "ci", "build"]);

/**
 * Formats a list of git commits into categorized markdown release notes.
 */
export function formatCommitChangelog(commits, { heading, maxItems = 20, compareUrl } = {}) {
  const entries = (Array.isArray(commits) ? commits : [])
    .map((commit) => ({
      sha: typeof commit?.sha === "string" ? commit.sha.trim() : "",
      subject: typeof commit?.subject === "string" ? commit.subject.trim() : "",
    }))
    .filter((commit) => commit.subject && !/^Merge\b/i.test(commit.subject));

  const features = [];
  const fixes = [];
  const improvements = [];
  const fallback = [];

  for (const entry of entries) {
    const match = conventionalPattern.exec(entry.subject);
    const shortSha = entry.sha ? ` (\`${entry.sha.slice(0, 7)}\`)` : "";
    if (!match) {
      fallback.push(`- ${entry.subject}${shortSha}`);
      continue;
    }
    const type = match[1].toLowerCase();
    const scope = match[2]?.trim();
    const description = match[3].trim();
    const formatted = scope
      ? `- **${scope}**: ${description}${shortSha}`
      : `- ${description}${shortSha}`;

    if (type === "feat") features.push(formatted);
    else if (type === "fix") fixes.push(formatted);
    else if (type === "perf" || type === "refactor" || type === "docs") improvements.push(formatted);
    else if (type === "chore") {
      if (scope?.toLowerCase() !== "release" && !/^bump version/i.test(description)) {
        fallback.push(formatted);
      }
    } else if (!noisyTypes.has(type)) {
      fallback.push(formatted);
    }
  }

  const sections = [];
  if (features.length) sections.push(["### Features", features]);
  if (fixes.length) sections.push(["### Fixes", fixes]);
  if (improvements.length) sections.push(["### Improvements", improvements]);

  if (!sections.length && fallback.length) {
    sections.push(["### Changes", fallback]);
  }

  const lines = [];
  if (heading?.trim()) lines.push(heading.trim(), "");

  let remaining = Math.max(1, maxItems);
  let omitted = 0;

  for (const [title, items] of sections) {
    if (remaining <= 0) {
      omitted += items.length;
      continue;
    }
    const taken = items.slice(0, remaining);
    omitted += items.length - taken.length;
    remaining -= taken.length;
    lines.push(title, ...taken, "");
  }

  if (omitted > 0) {
    lines.push(`- ...and ${omitted} more change${omitted === 1 ? "" : "s"}.`, "");
  }

  if (compareUrl?.trim()) {
    lines.push(`[Full Changelog](${compareUrl.trim()})`);
  }

  return lines.join("\n").trim();
}

/**
 * Removes automated platform installation appendix markers from release notes.
 */
export function stripPlatformNotesMarkers(markdown) {
  if (typeof markdown !== "string") return "";
  return markdown
    .replace(/\r\n/g, "\n")
    .replace(/<!--\s*vrcs-platform-notes-start\s*-->[\s\S]*?<!--\s*vrcs-platform-notes-end\s*-->/g, "")
    .trim();
}
