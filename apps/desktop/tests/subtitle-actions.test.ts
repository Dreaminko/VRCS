import assert from "node:assert/strict";
import test from "node:test";

import { combineSubtitleText, subtitleCopyText, subtitleSelectionCopyText } from "../src/subtitle-actions.ts";
import type { Subtitle } from "../src/types.ts";

test("builds concise subtitle clipboard payloads", () => {
  assert.equal(subtitleCopyText("  原文  ", null, "original"), "原文");
  assert.equal(subtitleCopyText("原文", "  translation  ", "translation"), "translation");
  assert.equal(subtitleCopyText("原文", "translation", "bilingual"), "原文\ntranslation");
  assert.equal(subtitleCopyText("原文", null, "bilingual"), "原文");
});

test("combines selected subtitle fragments in chronological order", () => {
  const subtitles = [
    subtitle({ id: 2, text: "go home.", created_at: "2026-01-01T00:00:02Z" }),
    subtitle({ id: 1, text: "I want to", created_at: "2026-01-01T00:00:01Z" }),
  ];
  assert.equal(combineSubtitleText(subtitles), "I want to go home.");
  assert.equal(subtitleSelectionCopyText(subtitles, "original"), "I want to go home.");
});

test("combines compact scripts without spaces and preserves speaker boundaries", () => {
  assert.equal(combineSubtitleText([
    subtitle({ id: 1, text: "今日は", language: "ja", created_at: "2026-01-01T00:00:01Z" }),
    subtitle({ id: 2, text: "いい天気です。", language: "ja", created_at: "2026-01-01T00:00:02Z" }),
  ]), "今日はいい天気です。");
  assert.equal(combineSubtitleText([
    subtitle({ id: 1, text: "hello", source: "speaker", created_at: "2026-01-01T00:00:01Z" }),
    subtitle({ id: 2, text: "hi", source: "microphone", created_at: "2026-01-01T00:00:02Z" }),
  ]), "hello\nhi");
});

function subtitle(overrides: Partial<Subtitle>): Subtitle {
  return {
    id: 1,
    text: "text",
    language: "en",
    started_at: null,
    ended_at: null,
    created_at: "2026-01-01T00:00:00Z",
    source: "speaker",
    translations: [],
    ...overrides,
  };
}
