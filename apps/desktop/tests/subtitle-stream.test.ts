import assert from "node:assert/strict";
import test from "node:test";

import {
  conversationSubtitlePage,
  isAbortError,
  isConversationRequestCurrent,
  MAX_SUBTITLE_HISTORY_ITEMS,
  MAX_SUBTITLE_HISTORY_TEXT_CHARS,
  mergeSubtitleHistory,
  parseSubtitleStreamMessage,
  upsertSubtitleHistory,
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

test("conversation pages preserve the Core pagination cursor", () => {
  const page = conversationSubtitlePage({
    items: Array.from({ length: 100 }, (_, index) => subtitle(100 - index, `line ${index}`)),
    has_more: true,
    next_before_id: 1,
  });
  assert.equal(page.items.length, 100);
  assert.equal(page.hasOlder, true);
  assert.equal(page.nextBeforeId, 1);

  assert.deepEqual(conversationSubtitlePage({
    items: [subtitle(1, "last")],
    has_more: false,
    next_before_id: 1,
  }), {
    items: [subtitle(1, "last")],
    hasOlder: false,
    nextBeforeId: null,
  });
});

test("rapid switches invalidate requests from the previous conversation", () => {
  const request = { conversationId: "first", version: 4 };
  assert.equal(isConversationRequestCurrent(request, "first", 4), true);
  assert.equal(isConversationRequestCurrent(request, "second", 5), false);
  assert.equal(isConversationRequestCurrent(request, "first", 5), false);
});

test("streamed subtitles are inserted without rebuilding history", () => {
  const current = [subtitle(3, "three"), subtitle(1, "one")];
  assert.deepEqual(
    upsertSubtitleHistory(current, subtitle(2, "two")).map((item) => item.id),
    [3, 2, 1],
  );
  const replaced = upsertSubtitleHistory(current, subtitle(3, "updated"));
  assert.equal(replaced[0]?.text, "updated");
  assert.equal(replaced[1], current[1]);
});

test("a late history snapshot cannot erase a streamed subtitle", () => {
  assert.deepEqual(
    mergeSubtitleHistory([subtitle(2, "streamed")], [subtitle(1, "snapshot")])
      .map((item) => item.id),
    [2, 1],
  );
});

test("conversation pools keep a bounded window of the newest subtitles", () => {
  const expanded = mergeSubtitleHistory(
    [],
    Array.from(
      { length: MAX_SUBTITLE_HISTORY_ITEMS + 1 },
      (_, index) => subtitle(MAX_SUBTITLE_HISTORY_ITEMS + 1 - index, `line ${index}`),
    ),
  );
  assert.equal(expanded.length, MAX_SUBTITLE_HISTORY_ITEMS);
  assert.equal(expanded[0]?.id, MAX_SUBTITLE_HISTORY_ITEMS + 1);
  assert.equal(expanded.at(-1)?.id, 2);
});

test("conversation pools enforce a text budget", () => {
  const text = "x".repeat(100_000);
  const expanded = mergeSubtitleHistory(
    [],
    Array.from({ length: 100 }, (_, index) => subtitle(100 - index, text)),
  );
  assert.ok(expanded.length < 100);
  assert.ok(
    expanded.reduce((total, item) => total + item.text.length, 0)
      <= MAX_SUBTITLE_HISTORY_TEXT_CHARS,
  );
});

test("stream updates evict the oldest subtitle when the pool is full", () => {
  const current = Array.from(
    { length: MAX_SUBTITLE_HISTORY_ITEMS },
    (_, index) => subtitle(MAX_SUBTITLE_HISTORY_ITEMS - index, `line ${index}`),
  );
  const updated = upsertSubtitleHistory(
    current,
    subtitle(MAX_SUBTITLE_HISTORY_ITEMS + 1, "newest"),
  );
  assert.equal(updated.length, MAX_SUBTITLE_HISTORY_ITEMS);
  assert.equal(updated[0]?.id, MAX_SUBTITLE_HISTORY_ITEMS + 1);
  assert.equal(updated.at(-1)?.id, 2);
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

test("complete conversation catalogs pass protocol validation", () => {
  const catalog = {
    conversations: [{
      id: "conversation-1",
      started_at: "2026-08-11T00:00:00Z",
      ended_at: null,
      automatic_title: "Authoritative title",
      custom_title: null,
      icon: null,
      subtitle_count: 101,
      updated_at: "2026-08-11T00:01:00Z",
      active: true,
    }],
  };
  assert.deepEqual(
    parseSubtitleStreamMessage(JSON.stringify({
      type: "conversation_catalog",
      catalog,
    })),
    { type: "conversation_catalog", catalog },
  );
});

test("malformed conversation catalogs are rejected", () => {
  assert.equal(parseSubtitleStreamMessage(JSON.stringify({
    type: "conversation_catalog",
    catalog: {
      conversations: [{ id: "conversation-1", subtitle_count: -1 }],
    },
  })), null);
});

test("aborted history requests are recognized without reporting an error", () => {
  const controller = new AbortController();
  controller.abort();
  assert.equal(isAbortError(controller.signal.reason), true);
  assert.equal(isAbortError(new Error("network failed")), false);
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
  const streamed = { ...subtitle(3, "valid"), conversation_id: "conversation-1" };
  const message = parseSubtitleStreamMessage(JSON.stringify({
    type: "subtitle",
    subtitle: streamed,
  }));
  assert.deepEqual(message, { type: "subtitle", subtitle: streamed });
});

test("OpenAI-compatible translation payloads pass protocol validation", () => {
  const translated = {
    ...subtitle(3, "valid"),
    translations: [{
      text: "translated",
      source_language: "ja",
      target_language: "en",
      provider: "openai_compatible" as const,
      model: "deepseek-chat",
      created_at: "2026-08-11T00:02:00Z",
    }],
  };
  const message = parseSubtitleStreamMessage(JSON.stringify({
    type: "subtitle",
    subtitle: translated,
  }));
  assert.deepEqual(message, { type: "subtitle", subtitle: translated });
});

test("Chatbox messages pass subtitle stream validation", () => {
  const outgoing = { ...subtitle(4, "hello"), source: "chatbox" as const };
  const message = parseSubtitleStreamMessage(JSON.stringify({
    type: "subtitle",
    subtitle: outgoing,
  }));
  assert.deepEqual(message, { type: "subtitle", subtitle: outgoing });
});

test("microphone audio levels pass protocol validation", () => {
  const message = parseSubtitleStreamMessage(JSON.stringify({
    type: "audio_level",
    source: "microphone",
    rms_dbfs: -42.5,
    peak_dbfs: -31.2,
    speech: true,
  }));
  assert.deepEqual(message, {
    type: "audio_level",
    source: "microphone",
    rms_dbfs: -42.5,
    peak_dbfs: -31.2,
    speech: true,
  });
});

test("VRChat mute status events pass protocol validation", () => {
  const message = parseSubtitleStreamMessage(JSON.stringify({
    type: "vrchat_mute_status",
    status: {
      enabled: true,
      connection: "connected",
      muted: true,
      last_error: null,
    },
  }));
  assert.deepEqual(message, {
    type: "vrchat_mute_status",
    status: {
      enabled: true,
      connection: "connected",
      muted: true,
      last_error: null,
    },
  });
});

test("invalid VRChat mute states are rejected", () => {
  assert.equal(parseSubtitleStreamMessage(JSON.stringify({
    type: "vrchat_mute_status",
    status: {
      enabled: true,
      connection: "connected",
      muted: "yes",
      last_error: null,
    },
  })), null);
});

test("out-of-range microphone levels are rejected", () => {
  assert.equal(parseSubtitleStreamMessage(JSON.stringify({
    type: "audio_level",
    source: "microphone",
    rms_dbfs: -81,
    peak_dbfs: -31.2,
    speech: false,
  })), null);
});
