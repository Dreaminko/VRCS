export type CompactFontSize = "small" | "medium" | "large";

export const COMPACT_FONT_PRESETS = {
  small: { original: 16, originalLine: 21, translation: 14, translationLine: 19 },
  medium: { original: 19, originalLine: 25, translation: 17, translationLine: 23 },
  large: { original: 23, originalLine: 30, translation: 20, translationLine: 27 },
} as const;

export const COMPACT_LAYOUT = {
  headerHeight: 32,
  headerPaddingLeft: 12,
  headerPaddingRight: 8,
  bodyPaddingTop: 6,
  bodyPaddingBottom: 8,
  bodyPaddingX: 12,
  actionWidth: 40,
  columnGap: 8,
  groupGap: 6,
  languageGap: 2,
  headerButtonSize: 26,
  headerButtonGap: 4,
  menuWidth: 120,
  menuPadding: 4,
  menuItemHeight: 30,
  menuGap: 4,
  captureButtonSize: 36,
  resizeGripSize: 20,
  resizeGripInset: 2,
} as const;

/** Heights are natural, unclipped measurements, ordered oldest to newest. */
export function fitRecentGroups(
  heights: readonly number[],
  availableHeight: number,
  gap: number = COMPACT_LAYOUT.groupGap,
): number {
  let count = 0;
  let used = 0;
  for (let index = heights.length - 1; index >= 0 && availableHeight > 0; index--) {
    const next = used + (count ? gap : 0) + heights[index];
    if (next > availableHeight) break;
    used = next;
    count++;
  }
  return count;
}

export interface CompactTextBudget {
  original: number;
  translation: number;
}

/** Only the oversized newest group receives paragraph height limits. */
export function allocateLatestLines(
  natural: CompactTextBudget,
  availableHeight: number,
  fontSize: CompactFontSize,
): CompactTextBudget {
  const preset = COMPACT_FONT_PRESETS[fontSize];
  const lines = { original: preset.originalLine, translation: preset.translationLine };
  const budget: CompactTextBudget = { original: 0, translation: 0 };
  let remaining = Math.max(0, availableHeight - (
    natural.original > 0 && natural.translation > 0 ? COMPACT_LAYOUT.languageGap : 0
  ));
  const sides = ["original", "translation"] as const;
  // Reserve one whole line for each language before distributing more space.
  for (const side of sides) {
    if (natural[side] > 0 && remaining >= lines[side]) {
      budget[side] = Math.min(lines[side], natural[side]);
      remaining -= budget[side];
    }
  }
  while (true) {
    const eligible = sides.filter((side) =>
      budget[side] + lines[side] <= natural[side] && remaining >= lines[side],
    );
    if (!eligible.length) break;
    const side = eligible.length === 2 && budget.translation < budget.original
      ? "translation" : eligible[0];
    budget[side] += lines[side];
    remaining -= lines[side];
  }
  return budget;
}
