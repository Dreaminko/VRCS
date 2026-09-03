import assert from "node:assert/strict";
import test from "node:test";

import {
  buildSubtitleLearningCapture,
  explanationLanguageForUiLocale,
  learningItemCaptureKeys,
  learningItemMatchesStatus,
  mergeLearningItemPages,
  normalizeLearningCardDraft,
} from "../src/learning.ts";
import type { LearningItem, Subtitle } from "../src/types.ts";

function subtitle(overrides: Partial<Subtitle> & Pick<Subtitle, "id" | "text" | "created_at">): Subtitle {
  return {
    id: overrides.id,
    text: overrides.text,
    language: overrides.language ?? "ja",
    started_at: null,
    ended_at: null,
    created_at: overrides.created_at,
    source: overrides.source ?? "speaker",
    translations: overrides.translations ?? [],
  };
}

function item(id: number, status: LearningItem["status"], createdAt: string, workingText = `item ${id}`): LearningItem {
  return {
    id,
    kind: "sentence",
    status,
    source_text: workingText,
    working_text: workingText,
    selected_text: null,
    source_translation: null,
    source_language: "ja",
    source_subtitle_ids: [id],
    dictionary_entries: [],
    analysis: null,
    draft: null,
    anki_note_id: null,
    created_at: createdAt,
    updated_at: createdAt,
  };
}

test("builds a chronological excerpt capture from arbitrary selected subtitles", () => {
  const subtitles = [
    subtitle({ id: 3, text: "三番目", created_at: "2026-01-01T00:00:03Z" }),
    subtitle({ id: 1, text: "一番目", created_at: "2026-01-01T00:00:01Z", translations: [{ text: "first", source_language: "ja", target_language: "en", provider: "local", model: null, created_at: "2026-01-01T00:00:01Z" }] }),
    subtitle({ id: 2, text: "二番目", created_at: "2026-01-01T00:00:02Z" }),
  ];

  const capture = buildSubtitleLearningCapture(subtitles, [3, 1]);

  assert.ok(capture);
  assert.equal(capture.kind, "excerpt");
  assert.equal(capture.source_text, "一番目\n三番目");
  assert.equal(capture.working_text, "一番目\n三番目");
  assert.equal(capture.source_translation, "first");
  assert.deepEqual(capture.source_subtitle_ids, [1, 3]);
});

test("builds a merged sentence capture for split live subtitles", () => {
  const capture = buildSubtitleLearningCapture([
    subtitle({ id: 2, text: "go home.", language: "en", created_at: "2026-01-01T00:00:02Z" }),
    subtitle({ id: 1, text: "I want to", language: "en", created_at: "2026-01-01T00:00:01Z" }),
  ], [2, 1], { mergeFragments: true });

  assert.equal(capture?.kind, "sentence");
  assert.equal(capture?.source_text, "I want to go home.");
  assert.deepEqual(capture?.source_subtitle_ids, [1, 2]);
});

test("builds a sentence capture for one subtitle and clears mixed language", () => {
  const single = buildSubtitleLearningCapture([
    subtitle({ id: 7, text: "hello", language: "en", created_at: "2026-01-01T00:00:00Z" }),
  ]);
  assert.equal(single?.kind, "sentence");
  assert.equal(single?.source_language, "en");

  const mixed = buildSubtitleLearningCapture([
    subtitle({ id: 8, text: "hello", language: "en", created_at: "2026-01-01T00:00:00Z" }),
    subtitle({ id: 9, text: "こんにちは", language: "ja", created_at: "2026-01-01T00:00:01Z" }),
  ]);
  assert.equal(mixed?.source_language, null);
});

test("normalizes editable draft fields without changing the card type", () => {
  assert.deepEqual(normalizeLearningCardDraft({
    card_type: "fill_blank",
    term: "  気になる  ",
    reading: "  きになる ",
    definition: "  在意  ",
    context: "  結果が気になる。  ",
    dictionary: "   ",
    language: " ja ",
  }), {
    card_type: "fill_blank",
    term: "気になる",
    reading: "きになる",
    definition: "在意",
    context: "結果が気になる。",
    dictionary: null,
    language: "ja",
  });
});

test("preserves the resolved interface language and script for AI explanations", () => {
  assert.equal(explanationLanguageForUiLocale("zh-CN"), "zh-CN");
  assert.equal(explanationLanguageForUiLocale("ja-JP"), "ja-JP");
  assert.equal(explanationLanguageForUiLocale("en-GB"), "en-GB");
  assert.equal(explanationLanguageForUiLocale("zh-Hant"), "zh-Hant");
  assert.equal(explanationLanguageForUiLocale("fr-FR"), "fr-FR");
  assert.equal(explanationLanguageForUiLocale(undefined), "en-US");
  assert.equal(explanationLanguageForUiLocale("invalid locale"), "en-US");
});

test("restores captured source keys from persisted learning items", () => {
  const sentence = item(8, "collected", "2026-01-01T00:00:00Z");
  sentence.source_subtitle_ids = [8, 9];
  assert.deepEqual(learningItemCaptureKeys(sentence), [
    "subtitle:8",
    "subtitle:9",
    "subtitles:8,9",
  ]);

  const word = item(10, "collected", "2026-01-01T00:00:00Z");
  word.kind = "word";
  word.source_text = "結果が気になる。";
  word.selected_text = "気になる";
  assert.deepEqual(learningItemCaptureKeys(word), [
    "subtitle:10",
    "lookup:10:気になる:結果が気になる。",
  ]);
});

test("merges status pages by id, keeps updates, and sorts newest first", () => {
  const firstPage = [
    item(4, "collected", "2026-01-04T00:00:00Z"),
    item(3, "collected", "2026-01-03T00:00:00Z"),
  ];
  const secondPage = [
    item(3, "analyzed", "2026-01-03T00:00:00Z", "updated"),
    item(2, "collected", "2026-01-02T00:00:00Z"),
  ];

  const merged = mergeLearningItemPages(firstPage, secondPage);

  assert.deepEqual(merged.map((candidate) => candidate.id), [4, 3, 2]);
  assert.equal(merged.find((candidate) => candidate.id === 3)?.status, "analyzed");
  assert.equal(merged.find((candidate) => candidate.id === 3)?.working_text, "updated");
  assert.equal(learningItemMatchesStatus(merged[1], "analyzed"), true);
  assert.equal(learningItemMatchesStatus(merged[1], "collected"), false);
  assert.equal(learningItemMatchesStatus(merged[1], "all"), true);
});
