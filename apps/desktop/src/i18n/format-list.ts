/** Format a human-readable conjunction list using locale-appropriate separators. */
export function formatLocalizedList(items: string[], locale: string): string {
  if (items.length === 0) return "";
  try {
    return new Intl.ListFormat(locale, { style: "narrow", type: "conjunction" }).format(items);
  } catch {
    return items.join(", ");
  }
}
