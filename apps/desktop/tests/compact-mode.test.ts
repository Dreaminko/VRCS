import assert from "node:assert/strict";
import test from "node:test";
import {
  COMPACT_LOOKUP_WINDOW_SIZE,
  COMPACT_WINDOW_MIN_WIDTH,
  COMPACT_WINDOW_SIZE,
  compactWindowConstraints,
  compactWindowSize,
  subtitleForCompactView,
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
];

test("compact mode follows the latest subtitle when lookup is closed", () => {
  assert.equal(subtitleForCompactView(subtitles), subtitles[0]);
});

test("compact mode freezes the selected subtitle while lookup is open", () => {
  assert.equal(subtitleForCompactView(subtitles, "selected subtitle"), subtitles[1]);
});

test("compact lookup expands the current window without changing its default width", () => {
  assert.deepEqual(compactWindowSize(false), COMPACT_WINDOW_SIZE);
  assert.deepEqual(compactWindowSize(true), COMPACT_LOOKUP_WINDOW_SIZE);
  assert.equal(COMPACT_WINDOW_SIZE.width, COMPACT_LOOKUP_WINDOW_SIZE.width);
  assert.ok(COMPACT_LOOKUP_WINDOW_SIZE.height > COMPACT_WINDOW_SIZE.height);
});

test("compact lookup preserves a user-resized width", () => {
  assert.deepEqual(compactWindowSize(false, 960), {
    width: 960,
    height: COMPACT_WINDOW_SIZE.height,
  });
  assert.deepEqual(compactWindowSize(true, 960), {
    width: 960,
    height: COMPACT_LOOKUP_WINDOW_SIZE.height,
  });
});

test("compact mode constrains only the minimum width and current height", () => {
  assert.deepEqual(compactWindowConstraints(false), {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: COMPACT_WINDOW_SIZE.height,
    maxHeight: COMPACT_WINDOW_SIZE.height,
  });
  assert.deepEqual(compactWindowConstraints(true), {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: COMPACT_LOOKUP_WINDOW_SIZE.height,
    maxHeight: COMPACT_LOOKUP_WINDOW_SIZE.height,
  });
  assert.equal("maxWidth" in compactWindowConstraints(false), false);
});
