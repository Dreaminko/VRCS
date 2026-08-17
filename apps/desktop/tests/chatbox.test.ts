import assert from "node:assert/strict";
import test from "node:test";

import {
  applyChatboxPreferences,
  chatboxPreferencesFromDraft,
  clearSentDraft,
  createChatboxDraft,
  normalizeChatboxPreferences,
  previewChatboxLocally,
} from "../src/chatbox.ts";
import { LatestWriteQueue } from "../src/latest-write-queue.ts";

test("local Chatbox preview mirrors the common message formats", () => {
  const draft = {
    ...createChatboxDraft("ja"),
    original: " hello\nworld ",
    translation: "こんにちは",
    send_mode: "bilingual" as const,
    message_format: "slash_separated" as const,
  };
  const preview = previewChatboxLocally(draft);
  assert.equal(preview.text, "hello world / こんにちは");
  assert.equal(preview.char_count, 19);
  assert.equal(preview.sendable, true);
});

test("sent drafts keep language and format preferences", () => {
  const draft = {
    ...createChatboxDraft("de"),
    original: "hello",
    translation: "hallo",
    send_mode: "bilingual" as const,
    message_format: "translation_newline_original" as const,
  };
  assert.deepEqual(clearSentDraft(draft), {
    ...draft,
    original: "",
    translation: null,
  });
});

test("new Chatbox drafts translate and send bilingually by default", () => {
  const draft = createChatboxDraft("zh-Hans");
  assert.equal(draft.send_mode, "bilingual");
  assert.equal(draft.target_language, "zh-Hans");
});

test("Chatbox send settings round-trip without retaining message text", () => {
  const draft = {
    ...createChatboxDraft("de"),
    original: "hello",
    translation: "hallo",
    send_mode: "translation" as const,
    message_format: "custom" as const,
    custom_format: "{translation} | {original}",
    overflow_policy: "smart_truncate" as const,
  };
  const restored = applyChatboxPreferences(
    createChatboxDraft("ja"),
    chatboxPreferencesFromDraft(draft),
  );
  assert.equal(restored.original, "");
  assert.equal(restored.translation, null);
  assert.equal(restored.target_language, "de");
  assert.equal(restored.send_mode, "translation");
  assert.equal(restored.message_format, "custom");
  assert.equal(restored.custom_format, "{translation} | {original}");
  assert.equal(restored.overflow_policy, "smart_truncate");
});

test("invalid persisted Chatbox settings fall back to current defaults", () => {
  assert.deepEqual(normalizeChatboxPreferences({
    target_language: "$invalid",
    send_mode: "invalid",
    message_format: "invalid",
    custom_format: 42,
    overflow_policy: "invalid",
  }, "zh-Hant"), chatboxPreferencesFromDraft(createChatboxDraft("zh-Hant")));
});

test("custom BCP 47 Chatbox languages survive preference round-trips", () => {
  const preferences = normalizeChatboxPreferences({
    target_language: "tlh-latn",
  }, "ja");
  assert.equal(preferences.target_language, "tlh-Latn");
});

test("Chatbox preference writes keep only the latest pending value", async () => {
  const writes: number[] = [];
  let releaseFirstWrite: (() => void) | undefined;
  const firstWrite = new Promise<void>((resolve) => {
    releaseFirstWrite = resolve;
  });
  const queue = new LatestWriteQueue<number>(async (value) => {
    writes.push(value);
    if (value === 1) await firstWrite;
  });

  const task = queue.enqueue(1);
  assert.equal(queue.enqueue(2), task);
  assert.equal(queue.enqueue(3), task);
  assert.deepEqual(writes, [1]);

  releaseFirstWrite?.();
  await task;

  assert.deepEqual(writes, [1, 3]);
});
