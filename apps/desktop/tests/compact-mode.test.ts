import assert from "node:assert/strict";
import test from "node:test";
import {
  beginDictionaryWindowRequest,
  COMPACT_WINDOW_SIZE,
  createDictionaryWindowLifecycle,
  detachedDictionaryPosition,
  dictionaryWindowOptions,
  dictionaryWindowUrl,
  isDictionaryWindow,
  isCurrentDictionaryWindowRequest,
  observeDictionaryWindowDestroyed,
  prepareDictionaryWindow,
  revealDictionaryWindow,
  subtitleForCompactView,
  trackDictionaryWindowRequest,
} from "../src/compact-mode.ts";
import type { Subtitle } from "../src/types.ts";

const subtitles: Subtitle[] = [
  {
    id: 2,
    text: "latest subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T10:01:00.000Z",
  },
  {
    id: 1,
    text: "selected subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T10:00:00.000Z",
  },
];

test("compact mode follows the latest subtitle when lookup is closed", () => {
  assert.equal(subtitleForCompactView(subtitles), subtitles[0]);
});

test("compact mode freezes the selected subtitle while lookup is open", () => {
  assert.equal(subtitleForCompactView(subtitles, "selected subtitle"), subtitles[1]);
});

test("dictionary lookup does not change the compact window size", () => {
  assert.deepEqual(COMPACT_WINDOW_SIZE, { width: 720, height: 120 });
});

test("dictionary window route does not depend on payload storage", () => {
  const url = new URL(dictionaryWindowUrl(), "https://vrcs.local/");
  assert.equal(isDictionaryWindow(url.search), true);
});

test("dictionary window stays hidden until its content is rendered", () => {
  assert.equal(dictionaryWindowOptions("話し · 词典").visible, false);
});

test("waits for the native move event before considering the dictionary positioned", async () => {
  const calls: string[] = [];
  let onMoved: ((event: { payload: { x: number; y: number } }) => void) | undefined;
  let positioned = false;
  const preparing = prepareDictionaryWindow({
    onMoved: async (handler) => {
      onMoved = handler;
      return () => calls.push("unlisten");
    },
    setPosition: async () => { calls.push("position"); },
    outerPosition: async () => ({ x: 0, y: 0 }),
  }, { x: 120, y: 240 }).then(() => { positioned = true; });

  await Promise.resolve();
  assert.equal(positioned, false);
  onMoved?.({ payload: { x: 120, y: 240 } });
  await preparing;
  assert.deepEqual(calls, ["position", "unlisten"]);
});

test("does not show the dictionary until native positioning is confirmed", async () => {
  let confirmPosition: () => void = () => {};
  const positionReady = new Promise<void>((resolve) => { confirmPosition = resolve; });
  const calls: string[] = [];
  const revealing = revealDictionaryWindow({
    show: async () => { calls.push("show"); },
  }, positionReady);

  await Promise.resolve();
  assert.deepEqual(calls, []);
  confirmPosition();
  await revealing;
  assert.deepEqual(calls, ["show"]);
});

test("a new lookup retires listeners from the previous dictionary window", () => {
  const lifecycle = createDictionaryWindowLifecycle();
  const calls: string[] = [];
  const first = beginDictionaryWindowRequest(lifecycle);
  trackDictionaryWindowRequest(lifecycle, first, () => calls.push("cleanup-first"));

  const second = beginDictionaryWindowRequest(lifecycle);
  trackDictionaryWindowRequest(lifecycle, second, () => calls.push("cleanup-second"));

  assert.deepEqual(calls, ["cleanup-first"]);
  assert.equal(isCurrentDictionaryWindowRequest(lifecycle, first), false);
  assert.equal(isCurrentDictionaryWindowRequest(lifecycle, second), true);
});

test("observes dictionary destruction without intercepting close requests", async () => {
  let observedEvent = "";
  let closed = false;
  const window = {
    once: async (event: string, handler: () => void) => {
      observedEvent = event;
      handler();
      return () => undefined;
    },
  };

  await observeDictionaryWindowDestroyed(window, () => { closed = true; });
  assert.equal(observedEvent, "tauri://destroyed");
  assert.equal(closed, true);
});

test("places the detached dictionary below or above without resizing the compact window", () => {
  const shared = {
    anchor: { top: 48, bottom: 72, centerX: 360 },
    windowPosition: { x: 100, y: 100 },
    monitorPosition: { x: 0, y: 0 },
    monitorSize: { width: 1920, height: 1080 },
    scaleFactor: 1,
  };
  assert.deepEqual(detachedDictionaryPosition(shared), { x: 270, y: 182 });
  assert.equal(detachedDictionaryPosition({ ...shared, windowPosition: { x: 100, y: 900 } }).y, 538);
});

test("keeps detached dictionary coordinates physical on a scaled secondary monitor", () => {
  assert.deepEqual(detachedDictionaryPosition({
    anchor: { top: 48, bottom: 72, centerX: 80 },
    windowPosition: { x: 2570, y: 75 },
    monitorPosition: { x: 2560, y: 0 },
    monitorSize: { width: 2560, height: 1440 },
    scaleFactor: 1.5,
  }), { x: 2560, y: 198 });
});
