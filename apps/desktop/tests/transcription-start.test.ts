import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_TRANSCRIPTION_START_BEHAVIOR,
  normalizeTranscriptionStartBehavior,
} from "../src/capture/transcription-start.ts";

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
