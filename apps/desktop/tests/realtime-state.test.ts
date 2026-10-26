import assert from "node:assert/strict";
import test from "node:test";

import {
  clearLivePartials,
  completeLivePartial,
  getLivePartial,
  getLivePartials,
  livePartialHasSubtitle,
  publishLivePartial,
  resetLivePartial,
} from "../src/core-client/realtime-state.ts";
import type { LiveTranscription } from "../src/capture/types.ts";
import { mergeSubtitleHistory } from "../src/subtitles/subtitle-stream.ts";
import type { Subtitle } from "../src/subtitles/types.ts";

function partial(utteranceId: string, text: string): LiveTranscription {
  return {
    type: "partial",
    utterance_id: utteranceId,
    source: "speaker",
    text,
    language: "en",
  };
}

test("simultaneous player previews update and complete independently", () => {
  clearLivePartials();
  const first = { ...partial("p1:1", "hello"), speaker: { id: "player-1", index: 0 } };
  const second = { ...partial("p2:1", "goodbye"), speaker: { id: "player-2", index: 1 } };
  publishLivePartial(first);
  publishLivePartial(second);
  const snapshot = getLivePartials("speaker");
  assert.equal(snapshot, getLivePartials("speaker"));
  assert.equal(snapshot.length, 2);
  publishLivePartial({ ...first, translation: "你好" });
  assert.equal(getLivePartials("speaker").length, 2);
  assert.equal(getLivePartials("speaker").find((p) => p.speaker?.id === "player-2"), snapshot[1]);
  completeLivePartial("speaker", first.utterance_id);
  publishLivePartial(first);
  assert.deepEqual(getLivePartials("speaker"), [snapshot[1]]);
  resetLivePartial("speaker");
  assert.deepEqual(getLivePartials("speaker"), []);
});

test("preview arrival time survives deltas but advances for a new utterance", (context) => {
  clearLivePartials();
  context.mock.timers.enable({ apis: ["Date"], now: 1_000 });
  const input = partial("first", "hello");
  publishLivePartial(input);
  assert.equal(getLivePartial("speaker")?.received_at, 1_000);
  assert.equal(input.received_at, undefined);
  context.mock.timers.tick(500);
  publishLivePartial({ ...input, text: "hello again" });
  assert.equal(getLivePartial("speaker")?.received_at, 1_000);
  publishLivePartial(partial("second", "new speech"));
  assert.equal(getLivePartial("speaker")?.received_at, 1_500);
  clearLivePartials();
});

test("player previews remain independent from the microphone", () => {
  clearLivePartials();
  publishLivePartial({ ...partial("mic", "me"), source: "microphone" });
  for (let i = 0; i < 30; i++) {
    publishLivePartial({ ...partial(`player-${i}:1`, "voice"), speaker: { id: `player-${i}`, index: i } });
  }
  assert.equal(getLivePartials("speaker").length, 30);
  resetLivePartial("speaker");
  assert.equal(getLivePartial("microphone")?.text, "me");
  clearLivePartials();
});

test("completing the current utterance clears its partial", () => {
  clearLivePartials();
  publishLivePartial(partial("utterance-1", "hello"));
  completeLivePartial("speaker", "utterance-1");

  assert.equal(getLivePartial("speaker"), null);
  clearLivePartials();
});

test("unlimited capture keeps more than eight concurrent player previews", () => {
  clearLivePartials();
  for (let index = 0; index < 40; index += 1) {
    publishLivePartial({ ...partial(`player-${index}`, `Player ${index}`), speaker: { id: `actor-${index}`, index } });
  }
  assert.equal(getLivePartials("speaker").length, 40);
  for (let index = 0; index < 39; index += 1) completeLivePartial("speaker", `player-${index}`);
  publishLivePartial({ ...partial("player-0", "late result"), speaker: { id: "actor-0", index: 0 } });
  assert.equal(getLivePartials("speaker").length, 1);
  assert.equal(getLivePartial("speaker")?.utterance_id, "player-39");
  clearLivePartials();
});

test("a late partial cannot reactivate a completed utterance", () => {
  clearLivePartials();
  publishLivePartial(partial("utterance-1", "hello"));
  completeLivePartial("speaker", "utterance-1");
  publishLivePartial(partial("utterance-1", "late"));

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

test("resetting one source permits an identifier in a new session", () => {
  clearLivePartials();
  publishLivePartial(partial("utterance-1", "first session"));
  completeLivePartial("speaker", "utterance-1");
  resetLivePartial("speaker");
  publishLivePartial(partial("utterance-1", "new session"));

  assert.equal(getLivePartial("speaker")?.text, "new session");
  clearLivePartials();
});

test("native translation can arrive first and clears with its session", () => {
  clearLivePartials();
  publishLivePartial({ ...partial("native-1", ""), translation: "你好", target_language: "zh-Hans" });
  assert.equal(getLivePartial("speaker")?.translation, "你好");
  publishLivePartial({ ...partial("native-1", "hello"), translation: "你好", target_language: "zh-Hans" });
  assert.equal(getLivePartial("speaker")?.text, "hello");
  completeLivePartial("speaker", "native-1");
  publishLivePartial({ ...partial("native-1", "late"), translation: "迟到" });
  assert.equal(getLivePartial("speaker"), null);
  clearLivePartials();
});

test("native deltas stay active after the original completes and link to one history row", () => {
  clearLivePartials();
  const preview = {
    ...partial("qwen-preview-1", "hello"),
    source_utterance_id: "qwen-source-1",
    translation: "预览",
    conversation_preview: { text: "full original", translation: "full preview" },
  };
  publishLivePartial(preview);
  completeLivePartial("speaker", "qwen-source-1");
  publishLivePartial({ ...preview, translation: "更新的预览" });
  assert.equal(getLivePartial("speaker")?.translation, "更新的预览");
  assert.equal(livePartialHasSubtitle(preview, []), false);
  const saved: Subtitle = {
    id: 1, utterance_id: "qwen-source-1", text: "full original",
    language: "en", source: "speaker", translations: [],
    created_at: "2026-10-04T00:00:00Z", started_at: null, ended_at: null,
  };
  assert.equal(livePartialHasSubtitle(preview, [saved]), true);
  assert.equal(livePartialHasSubtitle({ ...preview, source: "microphone" }, [saved]), false);
  assert.equal(livePartialHasSubtitle({ ...preview, source_utterance_id: "other" }, [saved]), false);
  const { utterance_id: _, ...snapshot } = saved;
  assert.equal(livePartialHasSubtitle(preview, mergeSubtitleHistory([snapshot], [saved])), true);
  completeLivePartial("speaker", "qwen-preview-1");
  publishLivePartial(preview);
  assert.equal(getLivePartial("speaker"), null);
  clearLivePartials();
});
