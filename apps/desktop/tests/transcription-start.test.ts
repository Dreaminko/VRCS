import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_TRANSCRIPTION_START_BEHAVIOR,
  normalizeTranscriptionStartBehavior,
  shouldCreateConversationOnCaptureToggle,
} from "../src/transcription-start.ts";

test("preserves the current conversation unless a new conversation is selected", () => {
  assert.equal(
    normalizeTranscriptionStartBehavior(undefined),
    DEFAULT_TRANSCRIPTION_START_BEHAVIOR,
  );
  assert.equal(
    normalizeTranscriptionStartBehavior("unknown"),
    DEFAULT_TRANSCRIPTION_START_BEHAVIOR,
  );
  assert.equal(
    normalizeTranscriptionStartBehavior("new_conversation"),
    "new_conversation",
  );
});

test("creates a conversation only when starting transcription in new-conversation mode", () => {
  assert.equal(
    shouldCreateConversationOnCaptureToggle(false, "new_conversation"),
    true,
  );
  assert.equal(
    shouldCreateConversationOnCaptureToggle(false, "continue_current"),
    false,
  );
  assert.equal(
    shouldCreateConversationOnCaptureToggle(true, "new_conversation"),
    false,
  );
});
