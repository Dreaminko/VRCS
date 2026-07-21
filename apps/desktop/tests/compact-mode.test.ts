import assert from "node:assert/strict";
import test from "node:test";
import { subtitleForCompactView } from "../src/compact-mode.ts";
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
