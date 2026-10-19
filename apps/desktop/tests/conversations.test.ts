import assert from "node:assert/strict";
import test from "node:test";

import {
  conversationsFromCatalog,
  matchingConversationTitles,
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

test("summary titles survive failures and remain below custom titles", () => {
  const source = catalog();
  source.conversations[0].summary_title = "Interview practice";
  source.conversations[0].title_generation_status = "failed";
  source.conversations[0].title_generation_error_code = "conversations.title_timeout";
  source.conversations[0].icon = "star";
  source.conversations[1].summary_title = "Earlier summary";

  const summaries = conversationsFromCatalog(source);
  assert.equal(summaries[0].title, "Interview practice");
  assert.equal(summaries[0].titleStatus, "failed");
  assert.equal(summaries[0].titleError, "conversations.title_timeout");
  assert.equal(summaries[0].hasCustomTitle, false);
  assert.equal(summaries[0].customized, true);
  assert.equal(summaries[1].title, "Custom title");
  source.conversations[1].custom_title = null;
  assert.equal(conversationsFromCatalog(source)[1].title, "Earlier summary");
});

test("old catalog entries default to idle and title searches preserve catalog order", () => {
  const source = catalog();
  const summaries = conversationsFromCatalog(source);
  assert.equal(summaries[0].titleStatus, "idle");
  assert.equal(summaries[0].summaryTitle, null);
  assert.equal(summaries[0].titleError, null);
  assert.deepEqual(matchingConversationTitles(summaries, "  TITLE  ").map(item => item.id), ["conversation-1", "conversation-0"]);
  assert.deepEqual(matchingConversationTitles(summaries, " "), []);
  assert.deepEqual(matchingConversationTitles(summaries, "missing"), []);
  source.conversations[0].summary_title = "日语敬语练习";
  assert.equal(matchingConversationTitles(conversationsFromCatalog(source), "敬语")[0]?.id, "conversation-1");
});
