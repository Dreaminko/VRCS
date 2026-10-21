import assert from "node:assert/strict";
import test from "node:test";
import {
  allocateLatestLines,
  COMPACT_FONT_PRESETS,
  COMPACT_LAYOUT,
  fitRecentGroups,
  type CompactFontSize,
} from "../src/shell/compact-layout.ts";

test("compact layout fits the newest contiguous groups including their gaps", () => {
  assert.equal(fitRecentGroups([40, 50, 30], 85), 1);
  assert.equal(fitRecentGroups([40, 50, 30], 86), 2);
  assert.equal(fitRecentGroups([40, 50, 30], 131), 2);
  assert.equal(fitRecentGroups([40, 50, 30], 132), 3);
  assert.equal(fitRecentGroups([40, 50, 30], 80, 0), 2);
  assert.equal(fitRecentGroups([20, 200, 30], 100), 1,
    "an older short group must not jump over a more recent group that does not fit");
});

test("compact layout reserves oversized latest groups for line allocation", () => {
  assert.equal(fitRecentGroups([20, 120], 112), 0);
  assert.equal(fitRecentGroups([], 112), 0);
  assert.equal(fitRecentGroups([25], 0), 0);
  assert.equal(fitRecentGroups([25], -1), 0);
});

test("compact capacity follows measured growth and can exceed four entries", () => {
  assert.equal(fitRecentGroups([25, 25, 25], 112), 3);
  assert.equal(fitRecentGroups([25, 25, 75], 112), 2);
  assert.equal(fitRecentGroups([25, 25, 100], 112), 1);
  assert.equal(fitRecentGroups([25, 25, 25, 25, 25], 149), 5);
  assert.equal(fitRecentGroups([25, 25, 25, 25, 25], 148), 4);
});

test("long bilingual subtitles receive complete lines at each font size", () => {
  assert.deepEqual(allocateLatestLines({ original: 126, translation: 114 }, 112, "small"),
    { original: 42, translation: 57 });
  assert.deepEqual(allocateLatestLines({ original: 150, translation: 138 }, 112, "medium"),
    { original: 50, translation: 46 });
  assert.deepEqual(allocateLatestLines({ original: 180, translation: 162 }, 112, "large"),
    { original: 30, translation: 54 });
});

test("line allocation gives spare space to the longer language without phantom gaps", () => {
  assert.deepEqual(allocateLatestLines({ original: 25, translation: 230 }, 112, "medium"),
    { original: 25, translation: 69 });
  assert.deepEqual(allocateLatestLines({ original: 250, translation: 23 }, 112, "medium"),
    { original: 75, translation: 23 });
  assert.deepEqual(allocateLatestLines({ original: 250, translation: 0 }, 100, "medium"),
    { original: 100, translation: 0 });
  assert.deepEqual(allocateLatestLines({ original: 0, translation: 230 }, 92, "medium"),
    { original: 0, translation: 92 });
  assert.deepEqual(allocateLatestLines({ original: 0, translation: 0 }, 112, "medium"),
    { original: 0, translation: 0 });
});

test("line budgets stay within the viewport and natural text heights across font sizes", () => {
  for (const fontSize of Object.keys(COMPACT_FONT_PRESETS) as CompactFontSize[]) {
    const preset = COMPACT_FONT_PRESETS[fontSize];
    for (const [originalLines, translationLines] of [[12, 9], [1, 9], [12, 1], [0, 9], [12, 0]]) {
      const natural = {
        original: originalLines * preset.originalLine,
        translation: translationLines * preset.translationLine,
      };
      const gap = originalLines && translationLines ? COMPACT_LAYOUT.languageGap : 0;
      for (let height = 0; height <= 720; height++) {
        const budget = allocateLatestLines(natural, height, fontSize);
        const context = `${fontSize}, ${originalLines}/${translationLines} lines, ${height}px`;
        assert.ok(budget.original >= 0 && budget.original <= natural.original, context);
        assert.ok(budget.translation >= 0 && budget.translation <= natural.translation, context);
        assert.equal(budget.original % preset.originalLine, 0, context);
        assert.equal(budget.translation % preset.translationLine, 0, context);
        assert.ok(budget.original + budget.translation <= Math.max(0, height - gap), context);
        if (originalLines && translationLines && height >= preset.originalLine + preset.translationLine + gap) {
          assert.ok(budget.original >= preset.originalLine, context);
          assert.ok(budget.translation >= preset.translationLine, context);
        }
      }
    }
  }
});
