import assert from "node:assert/strict";
import test from "node:test";

import {
  conversationsFromCatalog,
  type ConversationCatalog,
} from "../src/conversations/conversations.ts";

function catalog(): ConversationCatalog {
  return {
    conversations: [
      {
        id: "conversation-1",
        started_at: "2026-08-16T00:00:00Z",
        ended_at: null,
        automatic_title: "Core automatic title",
        custom_title: null,
        icon: null,
        subtitle_count: 42,
        updated_at: "2026-08-16T00:00:01Z",
        active: true,
      },
      {
        id: "conversation-0",
        started_at: "2026-08-15T00:00:00Z",
        ended_at: "2026-08-15T01:00:00Z",
        automatic_title: null,
        custom_title: "Custom title",
        icon: "study",
        subtitle_count: 7,
        updated_at: "2026-08-15T01:00:00Z",
        active: false,
      },
    ],
  };
}

test("catalog summaries use only Core-owned metadata", () => {
  const conversations = conversationsFromCatalog(catalog());

  assert.equal(conversations[0]?.title, "Core automatic title");
  assert.equal(conversations[1]?.title, "Custom title");
  assert.equal("subtitles" in conversations[0], false);
});

test("empty catalog titles use localized active and historical labels", () => {
  const source = catalog();
  source.conversations[0].automatic_title = null;
  source.conversations[0].subtitle_count = 0;
  source.conversations[1].custom_title = null;
  const conversations = conversationsFromCatalog(source, {
    untitled: "Untitled",
    newConversation: "New conversation",
  });

  assert.equal(conversations[0]?.title, "New conversation");
  assert.equal(conversations[1]?.title, "Untitled");
});

test("sidebar counts come from catalog subtitle_count", () => {
  const conversations = conversationsFromCatalog(catalog());

  assert.equal(conversations[0]?.subtitleCount, 42);
  assert.equal(conversations[1]?.subtitleCount, 7);
});
