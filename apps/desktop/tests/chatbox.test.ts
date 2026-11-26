import assert from "node:assert/strict";
import test from "node:test";

import {
  applyChatboxPreferences,
  chatboxPreferencesFromDraft,
  clearSentDraft,
  createChatboxDraft,
  isValidChatboxCustomFormat,
  normalizeChatboxPreferences,
  previewChatboxLocally,
  updateChatboxTranslation,
} from "../src/chatbox/chatbox.ts";
import { LatestWriteQueue } from "../src/shared/lib/latest-write-queue.ts";

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
    source_language: "en",
    send_mode: "bilingual" as const,
    message_format: "translation_newline_original" as const,
  };
  assert.deepEqual(clearSentDraft(draft), {
    ...draft,
    original: "",
    translation: null,
    translation_origin: null,
    source_language: null,
  });
});

test("PLaMo translations do not add a source marker", () => {
  const draft = {
    ...createChatboxDraft("ja"),
    original: "hello",
    translation: "こんにちは",
    translation_origin: {
      provider: "plamo_local",
      model: "plamo-3-translate-0.8b-q4_k_m",
    },
    send_mode: "bilingual" as const,
  };
  const preview = previewChatboxLocally(draft);
  assert.equal(preview.text, "hello\nこんにちは");
  assert.equal(preview.char_count, [...preview.text].length);
});

test("editing a translation clears its model origin", () => {
  const draft = {
    ...createChatboxDraft("ja"),
    translation: "こんにちは",
    translation_origin: {
      provider: "plamo_local",
      model: "plamo-3-translate-0.8b-q4_k_m",
    },
  };
  const edited = updateChatboxTranslation(draft, "こんにちは!");
  assert.equal(edited.translation, "こんにちは!");
  assert.equal(edited.translation_origin, null);
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
  assert.equal(restored.output_target, "both");
  assert.equal(restored.overflow_policy, "smart_truncate");

  const unitextOnly = applyChatboxPreferences(
    createChatboxDraft("ja"),
    chatboxPreferencesFromDraft({ ...restored, output_target: "unitext" }),
  );
  assert.equal(unitextOnly.output_target, "unitext");
});

test("following the preferred target survives saving and reopening", () => {
  const draft = createChatboxDraft("fr");
  const saved = JSON.stringify(chatboxPreferencesFromDraft(draft));
  const preferences = normalizeChatboxPreferences(saved, "de");
  assert.deepEqual(applyChatboxPreferences(createChatboxDraft(), preferences), draft);
});

test("new preferences and legacy follow mode follow the current preferred language", () => {
  for (const stored of [null, { target_language: null }]) {
    const prefs = normalizeChatboxPreferences(stored, "fr");
    assert.equal(prefs.follow_translation_settings, true);
    assert.equal(prefs.target_language, "fr");
  }
  assert.equal(normalizeChatboxPreferences({ target_language: null }, "zh-hans").target_language, "zh-Hans");
  assert.equal(normalizeChatboxPreferences({ target_language: null }, "$invalid").target_language, "ja");
});

test("custom Chatbox format accepts up to 200 characters and rejects longer templates", () => {
  const padding = "x".repeat(200 - "{original}{translation}".length);
  assert.equal(isValidChatboxCustomFormat(`{original}${padding}{translation}`), true);
  assert.equal(isValidChatboxCustomFormat(`{original}${padding}x{translation}`), false);
});

test("invalid persisted Chatbox settings fall back to current defaults", () => {
  assert.deepEqual(normalizeChatboxPreferences({
    target_language: "$invalid",
    send_mode: "invalid",
    overflow_policy: "invalid",
  }, "zh-Hant"), chatboxPreferencesFromDraft(createChatboxDraft("zh-Hant")));
});

test("custom BCP 47 Chatbox languages survive preference round-trips", () => {
  const preferences = normalizeChatboxPreferences({
    target_language: "tlh-latn",
  }, "ja");
  assert.equal(preferences.target_language, "tlh-Latn");
  assert.equal(preferences.follow_translation_settings, false);
  assert.deepEqual(normalizeChatboxPreferences(JSON.stringify(preferences)), preferences);
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

test("unbounded Chatbox preview lifts the 144-character limit for UniText-only output", () => {
  const longOriginal = "a".repeat(180);
  const longTranslation = "b".repeat(180);
  const draft = {
    ...createChatboxDraft("ja"),
    original: longOriginal,
    translation: longTranslation,
    send_mode: "bilingual" as const,
    message_format: "custom" as const,
    custom_format: "broken",
  };
  const preview = previewChatboxLocally(draft, { enforceOscLimit: false });
  assert.equal(preview.limit, null);
  assert.equal(preview.over_limit, false);
  assert.equal(preview.truncated, false);
  assert.equal(preview.sendable, true);
  assert.equal(preview.text, `${longOriginal}\n${longTranslation}`);
});
