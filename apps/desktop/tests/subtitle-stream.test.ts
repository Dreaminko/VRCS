import assert from "node:assert/strict";
import test from "node:test";

import {
  mergeSubtitleHistory,
  parseSubtitleStreamMessage,
} from "../src/subtitle-stream.ts";
import type { Subtitle } from "../src/types.ts";

function subtitle(id: number, text: string): Subtitle {
  return {
    id,
    text,
    language: "ja",
    started_at: null,
    ended_at: null,
    created_at: `2026-08-11T00:00:${String(id).padStart(2, "0")}Z`,
    source: "speaker",
    translations: [],
  };
}

test("a late history snapshot cannot erase a streamed subtitle", () => {
  assert.deepEqual(
    mergeSubtitleHistory([subtitle(2, "streamed")], [subtitle(1, "snapshot")])
      .map((item) => item.id),
    [2, 1],
  );
});

test("overlapping snapshots are deduplicated and keep the preferred version", () => {
  const merged = mergeSubtitleHistory(
    [subtitle(2, "new value")],
    [subtitle(2, "stale value"), subtitle(1, "older")],
  );
  assert.equal(merged.length, 2);
  assert.equal(merged[0]?.text, "new value");
});

test("a new stream event replaces the same subtitle from history", () => {
  const merged = mergeSubtitleHistory(
    [subtitle(2, "stream update")],
    [subtitle(2, "history value")],
  );
  assert.deepEqual(merged.map((item) => item.text), ["stream update"]);
});

test("a reconnect snapshot restores translations missed by the stream", () => {
  const live = {
    ...subtitle(2, "stream value"),
    translation_partial: {
      text: "translat",
      target_language: "en",
    },
  };
  const persisted = {
    ...subtitle(2, "snapshot value"),
    translations: [{
      text: "translated",
      source_language: "ja",
      target_language: "en",
      provider: "deepl" as const,
      model: null,
      created_at: "2026-08-11T00:01:00Z",
    }],
  };

  const [merged] = mergeSubtitleHistory([live], [persisted]);
  assert.equal(merged?.text, "stream value");
  assert.equal(merged?.translations[0]?.text, "translated");
  assert.equal(merged?.translation_partial, undefined);
});

test("translation reconciliation follows the database target-language identity", () => {
  const preferred = {
    ...subtitle(2, "stream value"),
    translations: [{
      text: "new provider value",
      source_language: "ja",
      target_language: "en",
      provider: "openai" as const,
      model: "gpt",
      created_at: "2026-08-11T00:02:00Z",
    }],
  };
  const fallback = {
    ...subtitle(2, "snapshot value"),
    translations: [{
      text: "old provider value",
      source_language: "ja",
      target_language: "en",
      provider: "deepl" as const,
      model: null,
      created_at: "2026-08-11T00:01:00Z",
    }],
  };

  const [merged] = mergeSubtitleHistory([preferred], [fallback]);
  assert.equal(merged?.translations.length, 1);
  assert.equal(merged?.translations[0]?.text, "new provider value");
});

test("malformed stream payloads are ignored at the protocol boundary", () => {
  assert.equal(parseSubtitleStreamMessage("{"), null);
  assert.equal(
    parseSubtitleStreamMessage(JSON.stringify({ type: "subtitle" })),
    null,
  );
  assert.equal(
    parseSubtitleStreamMessage(JSON.stringify({
      type: "translation_completed",
      subtitle_id: 1,
    })),
    null,
  );
  assert.equal(
    parseSubtitleStreamMessage(JSON.stringify({
      type: "partial",
      source: "__proto__",
      utterance_id: "1",
      text: "invalid source",
    })),
    null,
  );
  assert.equal(
    parseSubtitleStreamMessage(JSON.stringify({
      type: "partial",
      source: "microphone",
      utterance_id: "1",
      text: "invalid language",
      language: {},
    })),
    null,
  );
});

test("valid stream payloads pass protocol validation", () => {
  const message = parseSubtitleStreamMessage(JSON.stringify({
    type: "subtitle",
    subtitle: subtitle(3, "valid"),
  }));
  assert.equal(message?.type, "subtitle");
});
