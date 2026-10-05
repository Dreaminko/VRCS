import assert from "node:assert/strict";
import test from "node:test";

import { PREVIEW_INTERVAL_MS, createPreviewBatcher } from "../src/streaming-preview.ts";

function clock() {
  let time = 0;
  let id = 0;
  const tasks = new Map<number, { at: number; callback: () => void }>();
  return {
    schedule(callback: () => void, delay: number) {
      tasks.set(++id, { at: time + delay, callback });
      return id;
    },
    cancel(timer: unknown) { tasks.delete(timer as number); },
    advanceTo(until: number) {
      while (true) {
        const next = [...tasks.entries()].sort((a, b) => a[1].at - b[1].at)[0];
        if (!next || next[1].at > until) break;
        time = next[1].at;
        tasks.delete(next[0]);
        next[1].callback();
      }
      time = until;
    },
  };
}

test("a captured Qwen burst displays its latest bilingual snapshot after 50 ms", () => {
  const timers = clock();
  const displays: { text: string; translation: string }[] = [];
  const batcher = createPreviewBatcher((value: typeof displays[number]) => displays.push(value), timers);
  // The real en→zh capture delivered five translation deltas in 37 ms.
  const arrivals = [0, 8, 17, 28, 37];
  const parts = ["如果", "审计", "报告中提到的", "权限漏洞", "已经"];
  let translation = "";
  arrivals.forEach((at, index) => {
    timers.advanceTo(at);
    translation += parts[index];
    batcher.push({ text: "If the permission vulnerability", translation }, "qwen-preview-1");
  });
  timers.advanceTo(49);
  assert.deepEqual(displays, []);
  timers.advanceTo(50);
  assert.deepEqual(displays, [{ text: "If the permission vulnerability", translation }]);
  timers.advanceTo(1000);
  assert.equal(displays.length, 1);
});

test("continuous deltas cannot extend the window and keep updating during a long response", () => {
  const timers = clock();
  const displays: string[] = [];
  const batcher = createPreviewBatcher((text: string) => displays.push(text), timers);
  for (let at = 0; at <= 180; at += 20) {
    timers.advanceTo(at);
    batcher.push(`delta at ${at}`, "same");
  }
  assert.deepEqual(displays, ["delta at 40", "delta at 100", "delta at 160"]);
  timers.advanceTo(230);
  assert.deepEqual(displays, ["delta at 40", "delta at 100", "delta at 160", "delta at 180"]);
});

test("a delta at the end of a window is included without restarting the timer", () => {
  const timers = clock();
  const displays: string[] = [];
  const batcher = createPreviewBatcher((text: string) => displays.push(text), timers);
  batcher.push("first", "same");
  timers.advanceTo(PREVIEW_INTERVAL_MS - 1);
  batcher.push("latest", "same");
  timers.advanceTo(PREVIEW_INTERVAL_MS);
  assert.deepEqual(displays, ["latest"]);
});

test("switching utterances cancels the old preview instead of flashing it later", () => {
  const timers = clock();
  const displays: string[] = [];
  const batcher = createPreviewBatcher((text: string) => displays.push(text), timers);
  batcher.push("old utterance", "old");
  timers.advanceTo(20);
  batcher.push("new utterance", "new");
  timers.advanceTo(50);
  assert.deepEqual(displays, []);
  timers.advanceTo(70);
  assert.deepEqual(displays, ["new utterance"]);
});

test("completion or unmount cancels pending deltas without overwriting the final result", () => {
  const timers = clock();
  const displays: string[] = [];
  const batcher = createPreviewBatcher((text: string) => displays.push(text), timers);
  batcher.push("pending delta", "same");
  timers.advanceTo(20);
  batcher.cancel();
  displays.push("authoritative final result");
  timers.advanceTo(1000);
  assert.deepEqual(displays, ["authoritative final result"]);
});

test("the last sparse delta is displayed without needing another event", () => {
  const timers = clock();
  const displays: string[] = [];
  const batcher = createPreviewBatcher((text: string) => displays.push(text), timers);
  batcher.push("last delta", "same");
  timers.advanceTo(PREVIEW_INTERVAL_MS);
  assert.deepEqual(displays, ["last delta"]);
});
