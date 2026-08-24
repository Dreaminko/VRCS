import assert from "node:assert/strict";
import test from "node:test";
import {
  COMPACT_PANEL_WINDOW_SIZE,
  COMPACT_SUBTITLE_MAX_ITEMS,
  COMPACT_WINDOW_MAX_HEIGHT,
  COMPACT_WINDOW_MIN_WIDTH,
  COMPACT_WINDOW_SIZE,
  clampCompactWindowHeight,
  compactSubtitleCount,
  compactWindowConstraints,
  compactWindowSize,
  subtitlesForCompactView,
} from "../src/compact-mode.ts";
import type { Subtitle } from "../src/types.ts";

const subtitles: Subtitle[] = [
  {
    id: 2,
    text: "latest subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T10:01:00.000Z",
    translations: [],
  },
  {
    id: 1,
    text: "selected subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T10:00:00.000Z",
    translations: [],
  },
  {
    id: 0,
    text: "older subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T09:59:00.000Z",
    translations: [],
  },
  {
    id: null,
    text: "oldest subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T09:58:00.000Z",
    translations: [],
  },
];

test("compact mode follows the latest subtitle when the selection panel is closed", () => {
  assert.deepEqual(subtitlesForCompactView(subtitles, 120), [subtitles[0]]);
});

test("compact mode freezes the selected subtitle while the selection panel is open", () => {
  assert.deepEqual(
    subtitlesForCompactView(subtitles, 360, "selected subtitle"),
    [subtitles[1]],
  );
});

test("compact selection panel expands the current window without changing its default width", () => {
  assert.deepEqual(compactWindowSize(false), COMPACT_WINDOW_SIZE);
  assert.deepEqual(compactWindowSize(true), COMPACT_PANEL_WINDOW_SIZE);
  assert.equal(COMPACT_WINDOW_SIZE.width, COMPACT_PANEL_WINDOW_SIZE.width);
  assert.ok(COMPACT_PANEL_WINDOW_SIZE.height > COMPACT_WINDOW_SIZE.height);
});

test("compact selection panel preserves a user-resized width", () => {
  assert.deepEqual(compactWindowSize(false, 960, 240), {
    width: 960,
    height: 240,
  });
  assert.deepEqual(compactWindowSize(true, 960, 240), {
    width: 960,
    height: COMPACT_PANEL_WINDOW_SIZE.height,
  });
});

test("compact mode constrains width and normal subtitle height", () => {
  assert.deepEqual(compactWindowConstraints(false), {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: COMPACT_WINDOW_SIZE.height,
    maxHeight: COMPACT_WINDOW_MAX_HEIGHT,
  });
  assert.deepEqual(compactWindowConstraints(true), {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: COMPACT_PANEL_WINDOW_SIZE.height,
    maxHeight: COMPACT_PANEL_WINDOW_SIZE.height,
  });
  assert.equal("maxWidth" in compactWindowConstraints(false), false);
});

test("compact height is clamped to the supported range", () => {
  assert.equal(clampCompactWindowHeight(80), COMPACT_WINDOW_SIZE.height);
  assert.equal(clampCompactWindowHeight(241.6), 242);
  assert.equal(clampCompactWindowHeight(500), COMPACT_WINDOW_MAX_HEIGHT);
});

test("compact subtitle count increases at stable height steps", () => {
  assert.equal(compactSubtitleCount(120), 1);
  assert.equal(compactSubtitleCount(179), 1);
  assert.equal(compactSubtitleCount(180), 2);
  assert.equal(compactSubtitleCount(239), 2);
  assert.equal(compactSubtitleCount(240), 3);
  assert.equal(compactSubtitleCount(299), 3);
  assert.equal(compactSubtitleCount(300), COMPACT_SUBTITLE_MAX_ITEMS);
  assert.equal(compactSubtitleCount(360), COMPACT_SUBTITLE_MAX_ITEMS);
});

test("compact subtitle context is chronological and bounded by height", () => {
  assert.deepEqual(
    subtitlesForCompactView(subtitles, 240).map((subtitle) => subtitle.text),
    ["older subtitle", "selected subtitle", "latest subtitle"],
  );
  assert.deepEqual(
    subtitlesForCompactView([...subtitles, { ...subtitles[3], text: "beyond limit" }], 360)
      .map((subtitle) => subtitle.text),
    ["oldest subtitle", "older subtitle", "selected subtitle", "latest subtitle"],
  );
});
