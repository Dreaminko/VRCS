import assert from "node:assert/strict";
import test from "node:test";

import {
  isSubtitleSearchable,
  mergeSubtitleSearchHits,
  subtitleSearchMatchRange,
} from "../src/subtitle-search.ts";
import type { SubtitleSearchHit } from "../src/subtitles/types.ts";

function hit(id: number, text: string): SubtitleSearchHit {
  return {
    subtitle: {
      id,
      conversation_id: "conversation-1",
      text,
      language: "en",
      started_at: null,
      ended_at: null,
      created_at: `2026-08-22T00:00:${id.toString().padStart(2, "0")}Z`,
      translations: [],
    },
    matched_field: "original",
    matched_text: text,
  };
}

test("subtitle search accepts any non-empty query", () => {
  assert.equal(isSubtitleSearchable(""), false);
  assert.equal(isSubtitleSearchable("   "), false);
  assert.equal(isSubtitleSearchable("a"), true);
  assert.equal(isSubtitleSearchable("字"), true);
  assert.equal(isSubtitleSearchable("字幕"), true);
  assert.equal(isSubtitleSearchable("字幕検索"), true);
  assert.equal(isSubtitleSearchable("  abc  "), true);
});

test("search pagination preserves order and removes shifted duplicates", () => {
  const merged = mergeSubtitleSearchHits(
    [hit(3, "three"), hit(2, "two")],
    [hit(2, "updated but duplicate"), hit(1, "one")],
  );
  assert.deepEqual(merged.map((item) => item.subtitle.id), [3, 2, 1]);
  assert.equal(merged[1]?.matched_text, "two");
});

test("match highlighting is case insensitive and keeps source offsets", () => {
  assert.deepEqual(subtitleSearchMatchRange("Virtual Market", "market"), {
    start: 8,
    end: 14,
  });
  assert.equal(subtitleSearchMatchRange("Virtual Market", "missing"), null);
});
