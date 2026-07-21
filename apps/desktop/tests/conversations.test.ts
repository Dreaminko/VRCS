import assert from "node:assert/strict";
import test from "node:test";
import { conversationId, groupConversations } from "../src/conversations.ts";
import type { Subtitle } from "../src/types.ts";

function subtitle(id: number, createdAt: number, text: string): Subtitle {
  return {
    id,
    text,
    language: "zh",
    started_at: null,
    ended_at: null,
    created_at: new Date(createdAt).toISOString(),
  };
}

test("groups subtitles separated by thirty minutes into conversations", () => {
  const start = Date.parse("2026-07-21T08:00:00Z");
  const conversations = groupConversations([
    subtitle(3, start + 3_600_000, "第二次交流"),
    subtitle(2, start + 60_000, "继续第一次交流"),
    subtitle(1, start, "第一次交流"),
  ], [], start);

  assert.equal(conversations.length, 2);
  assert.equal(conversations[0].title, "第二次交流");
  assert.equal(conversations[1].subtitles.length, 2);
});

test("a manual start splits nearby subtitles", () => {
  const start = Date.parse("2026-07-21T08:00:00Z");
  const boundary = start + 120_000;
  const conversations = groupConversations([
    subtitle(2, start + 180_000, "新的对话"),
    subtitle(1, start, "之前的对话"),
  ], [boundary], start);

  assert.equal(conversations.length, 2);
  assert.equal(conversations[0].id, conversationId(boundary));
  assert.equal(conversations[0].subtitles[0].text, "新的对话");
});

test("keeps an empty latest conversation available", () => {
  const start = Date.parse("2026-07-21T08:00:00Z");
  const boundary = start + 120_000;
  const conversations = groupConversations([subtitle(1, start, "之前的对话")], [boundary], start);

  assert.equal(conversations[0].id, conversationId(boundary));
  assert.equal(conversations[0].title, "新对话");
  assert.equal(conversations[0].subtitles.length, 0);
});
