import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_VRCHAT_CHATBOX_CONFIG, chatboxTranslationReady } from "../src/settings/vrchat-chatbox-settings.ts";
import type { ApiProfileView } from "../src/providers/types.ts";
import type { TranslationSettings } from "../src/settings/types.ts";

const translation = {
  mode: "automatic",
  speaker_targets: [{ target_language: "en", profile_id: "text" }],
} as TranslationSettings;
const textProfile = { id: "text", enabled_capabilities: ["text_translation"], capabilities: { supports_translation: false } } as ApiProfileView;

test("Chatbox reception defaults to disabled independently of audio sources", () => {
  assert.deepEqual(DEFAULT_VRCHAT_CHATBOX_CONFIG, { enabled: false });
});

test("Chatbox automatic translation requires an other-player text translation route", () => {
  assert.equal(chatboxTranslationReady(translation, [textProfile]), true);
  assert.equal(chatboxTranslationReady({ ...translation, mode: "manual" }, [textProfile]), false);
  assert.equal(chatboxTranslationReady({ ...translation, mode: "disabled" }, [textProfile]), false);
  assert.equal(chatboxTranslationReady({ ...translation, speaker_targets: [] }, [textProfile]), false);
  assert.equal(chatboxTranslationReady(translation, []), false);
  assert.equal(chatboxTranslationReady(translation, [{ ...textProfile, enabled_capabilities: ["speech_to_text"] }]), false);
});
