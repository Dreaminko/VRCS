import assert from "node:assert/strict";
import test from "node:test";

import {
  activeConversationId,
  catalogAfterRequest,
  conversationSwitchDirection,
  conversationViewKey,
  getCachedConversationPage,
  MAX_CACHED_CONVERSATIONS,
  normalizeConversationTitle,
  selectedConversationIdForCatalog,
  setCachedConversationPage,
  updateCachedConversationPage,
} from "../src/conversations/conversation-state.ts";
import type { ConversationCatalog } from "../src/conversations/conversations.ts";
import type { Subtitle } from "../src/subtitles/types.ts";

test("normalizes whitespace and limits custom titles to forty characters", () => {
  assert.equal(normalizeConversationTitle("  VRChat   study  "), "VRChat study");
  assert.equal(normalizeConversationTitle(`  ${"对".repeat(44)}  `).length, 40);
});

function catalog(activeId: string, ids: string[]): ConversationCatalog {
  return {
    conversations: ids.map((id) => ({
      id,
      started_at: `2026-08-16T00:00:0${ids.indexOf(id)}Z`,
      ended_at: id === activeId ? null : "2026-08-16T00:01:00Z",
      automatic_title: id,
      custom_title: null,
      icon: null,
      subtitle_count: 1,
      updated_at: "2026-08-16T00:01:00Z",
      active: id === activeId,
    })),
  };
}

test("catalog selection preserves an existing conversation", () => {
  const next = catalog("current", ["current", "history"]);
  assert.equal(activeConversationId(next), "current");
  assert.equal(selectedConversationIdForCatalog(next, "history"), "history");
});

test("catalog selection falls back to active when the selection disappears", () => {
  const next = catalog("replacement", ["replacement", "history"]);
  assert.equal(selectedConversationIdForCatalog(next, "deleted"), "replacement");
  assert.equal(selectedConversationIdForCatalog({ conversations: [] }, "deleted"), null);
});

test("a catalog event received during an HTTP request remains authoritative", () => {
  const response = catalog("http", ["http"]);
  const eventCatalog = catalog("websocket", ["websocket"]);
  assert.equal(catalogAfterRequest(response, 4, {
    sequence: 5,
    catalog: eventCatalog,
  }), eventCatalog);
  assert.equal(catalogAfterRequest(response, 5, {
    sequence: 5,
    catalog: eventCatalog,
  }), response);
});

function subtitle(id: number, text = `msg-${id}`): Subtitle {
  return {
    id,
    text,
    language: "en",
    started_at: null,
    ended_at: null,
    created_at: "2026-08-16T00:00:00Z",
    translations: [],
  };
}

test("conversation view keys distinguish latest and focused subtitle views", () => {
  assert.equal(conversationViewKey(null), null);
  assert.equal(conversationViewKey("conv-1"), "conv-1:latest");
  assert.equal(conversationViewKey("conv-1", 42), "conv-1:at:42");
});

test("conversation switch direction follows sidebar list order", () => {
  const ordered = ["conv-1", "conv-2", "conv-3"];
  assert.equal(conversationSwitchDirection(ordered, "conv-1", "conv-2"), "down");
  assert.equal(conversationSwitchDirection(ordered, "conv-3", "conv-1"), "up");
  assert.equal(conversationSwitchDirection(ordered, "conv-2", "conv-2"), "neutral");
  assert.equal(conversationSwitchDirection(ordered, null, "conv-2"), "neutral");
  assert.equal(conversationSwitchDirection(ordered, "conv-1", "missing"), "neutral");
});

test("LRU conversation page cache evicts oldest untouched entry and updates in place", () => {
  const cache = new Map();
  for (let i = 1; i <= MAX_CACHED_CONVERSATIONS; i += 1) {
    setCachedConversationPage(cache, `conv-${i}`, {
      items: [subtitle(i)],
      hasOlder: false,
      nextBeforeId: null,
    });
  }
  // Touch conv-1 so conv-2 becomes the oldest entry.
  assert.equal(getCachedConversationPage(cache, "conv-1")?.items[0]?.id, 1);

  setCachedConversationPage(cache, "conv-extra", {
    items: [subtitle(99)],
    hasOlder: true,
    nextBeforeId: 50,
  });

  assert.equal(cache.size, MAX_CACHED_CONVERSATIONS);
  assert.equal(getCachedConversationPage(cache, "conv-2"), undefined);
  assert.equal(getCachedConversationPage(cache, "conv-1")?.items[0]?.id, 1);

  updateCachedConversationPage(cache, "conv-extra", (items) => [subtitle(100), ...items]);
  assert.deepEqual(
    getCachedConversationPage(cache, "conv-extra")?.items.map((item) => item.id),
    [100, 99],
  );
});
