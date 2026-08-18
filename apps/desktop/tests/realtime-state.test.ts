import assert from "node:assert/strict";
import test from "node:test";

import {
  clearLivePartials,
  completeLivePartial,
  getLivePartial,
  publishLivePartial,
} from "../src/realtime-state.ts";
import type { LiveTranscription } from "../src/types.ts";

function partial(utteranceId: string, text: string): LiveTranscription {
  return {
    type: "partial",
    utterance_id: utteranceId,
    source: "speaker",
    text,
    language: "en",
  };
}

test("completing the current utterance clears its partial", () => {
  clearLivePartials();
  publishLivePartial(partial("utterance-1", "hello"));
  completeLivePartial("speaker", "utterance-1");

  assert.equal(getLivePartial("speaker"), null);
  clearLivePartials();
});

test("completing an older utterance preserves the current partial", () => {
  clearLivePartials();
  publishLivePartial(partial("utterance-1", "first"));
  publishLivePartial(partial("utterance-2", "second"));
  completeLivePartial("speaker", "utterance-1");

  assert.equal(getLivePartial("speaker")?.utterance_id, "utterance-2");
  assert.equal(getLivePartial("speaker")?.text, "second");
  clearLivePartials();
});
