import assert from "node:assert/strict";
import test from "node:test";

import {
  mergeConversationStarts,
  normalizeConversationState,
  normalizeConversationTitle,
} from "../src/conversation-state.ts";
import { CONVERSATION_ICON_KEYS } from "../src/conversations.ts";

test("normalizes stored conversation customizations", () => {
  const state = normalizeConversationState({
    starts: [30, 10, 30, Number.NaN],
    customizations: {
      "conversation-10": { title: "  VRChat   学习会  ", icon: "study" },
      "conversation-30": { icon: "trophy" },
      "conversation-20": { title: "   ", icon: "unknown" },
      invalid: { title: "ignored" },
    },
  });

  assert.deepEqual(state.starts, [10, 30]);
  assert.deepEqual(state.customizations, {
    "conversation-10": { title: "VRChat 学习会", icon: "study" },
    "conversation-30": { title: undefined, icon: "trophy" },
  });
});

test("merges discovered boundaries without replacing an unchanged array", () => {
  const current = [10, 20];
  assert.equal(mergeConversationStarts(current, [20, 10]), current);
  assert.deepEqual(mergeConversationStarts(current, [30]), [10, 20, 30]);
});

test("accepts every selectable conversation icon", () => {
  const customizations = Object.fromEntries(CONVERSATION_ICON_KEYS.map((icon, index) => [
    `conversation-${index}`,
    { icon },
  ]));
  const state = normalizeConversationState({ customizations });

  assert.deepEqual(
    Object.values(state.customizations).map(({ icon }) => icon),
    CONVERSATION_ICON_KEYS,
  );
});

test("limits custom titles to forty characters", () => {
  assert.equal(normalizeConversationTitle(`  ${"对".repeat(44)}  `).length, 40);
});
