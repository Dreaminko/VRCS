import assert from "node:assert/strict";
import test from "node:test";

import {
  activeConversationId,
  catalogAfterRequest,
  normalizeConversationTitle,
  selectedConversationIdForCatalog,
} from "../src/conversation-state.ts";
import type { ConversationCatalog } from "../src/conversations.ts";

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
