import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_CONVERSATION_SIDEBAR_WIDTH,
  MAX_CONVERSATION_SIDEBAR_WIDTH,
  MIN_CONVERSATION_SIDEBAR_WIDTH,
  normalizeConversationSidebarWidth,
} from "../src/conversation-sidebar-width.ts";

test("normalizes the conversation sidebar width within its supported range", () => {
  assert.equal(
    normalizeConversationSidebarWidth("not-a-number"),
    DEFAULT_CONVERSATION_SIDEBAR_WIDTH,
  );
  assert.equal(normalizeConversationSidebarWidth(120), MIN_CONVERSATION_SIDEBAR_WIDTH);
  assert.equal(normalizeConversationSidebarWidth(900), MAX_CONVERSATION_SIDEBAR_WIDTH);
  assert.equal(normalizeConversationSidebarWidth(333.6), 334);
});

test("keeps a user-selected width independent of the window width", () => {
  assert.equal(normalizeConversationSidebarWidth(176), 176);
  assert.equal(normalizeConversationSidebarWidth(412), 412);
});
