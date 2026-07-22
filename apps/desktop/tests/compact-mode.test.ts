import assert from "node:assert/strict";
import test from "node:test";
import {
  COMPACT_LOOKUP_WINDOW_SIZE,
  COMPACT_WINDOW_SIZE,
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
  },
  {
    id: 1,
    text: "selected subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T10:00:00.000Z",
  },
];

test("compact mode follows the latest subtitle when lookup is closed", () => {
  assert.equal(subtitleForCompactView(subtitles), subtitles[0]);
});

test("compact mode freezes the selected subtitle while lookup is open", () => {
  assert.equal(subtitleForCompactView(subtitles, "selected subtitle"), subtitles[1]);
});

test("compact lookup expands the current window without changing its width", () => {
  assert.deepEqual(compactWindowSize(false), COMPACT_WINDOW_SIZE);
  assert.deepEqual(compactWindowSize(true), COMPACT_LOOKUP_WINDOW_SIZE);
  assert.equal(COMPACT_WINDOW_SIZE.width, COMPACT_LOOKUP_WINDOW_SIZE.width);
  assert.ok(COMPACT_LOOKUP_WINDOW_SIZE.height > COMPACT_WINDOW_SIZE.height);
});
