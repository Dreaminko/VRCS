import assert from "node:assert/strict";
import test from "node:test";

import {
  clearSentDraft,
  createChatboxDraft,
  previewChatboxLocally,
} from "../src/chatbox.ts";

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
