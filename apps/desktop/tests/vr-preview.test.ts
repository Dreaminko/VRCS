import assert from "node:assert/strict";
import test from "node:test";
import { texturePoint } from "../src/vr-preview/coordinates.ts";
import { previewQueue } from "../src/vr-preview/queue.ts";

test("preview queue coalesces motion while preserving Down and Up order", async () => {
  const calls: number[] = [];
  let release!: () => void;
  const first = new Promise<void>((resolve) => { release = resolve; });
  const queue = previewQueue(async (op) => {
    const value = (op.input as { buttons: number }).buttons;
    calls.push(value);
    if (calls.length === 1) await first;
  });
  const push = (buttons: number) => queue.push({ type: "pointer", input: { buttons, panel:0,generation:1,revision:1 } });
  push(0); push(0); push(0); push(1); push(1); push(0);
  release(); await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(calls,[0,0,1,0]);
  queue.close();
});

test("cancellation discards pending presses before changing panels", async () => {
  const calls: string[] = [];
  let release!: () => void;
  const first = new Promise<void>((resolve) => { release = resolve; });
  const queue = previewQueue(async (op) => { calls.push(op.type); if (calls.length === 1) await first; });
  queue.push({type:"refresh"});
  queue.push({type:"pointer",input:{buttons:1}});
  queue.push({type:"pointer",input:{buttons:0}});
  queue.push({type:"cancel"});
  queue.push({type:"select",panel:2});
  release(); await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(calls,["refresh","cancel","select"]);
  queue.close();
});

test("equal-scale preview maps texture coordinates and rejects letterboxing", () => {
  const box = { width: 1000, height: 500 };
  const texture = { width: 768, height: 768 };
  assert.deepEqual(texturePoint(250, 0, box, texture), [0, 0]);
  assert.deepEqual(texturePoint(500, 250, box, texture), [0.5, 0.5]);
  assert.equal(texturePoint(249, 250, box, texture), null);
  assert.equal(texturePoint(750, 250, box, texture), null);
  assert.equal(texturePoint(500, 250, { width: 0, height: 0 }, texture), null);
  assert.equal(texturePoint(Number.NaN, 1, box, texture), null);
});

test("React preview maps wide menu and portrait subtitle surfaces at equal scale", () => {
  const box = { width: 800, height: 600 };
  assert.deepEqual(texturePoint(400, 300, box, { width: 1024, height: 256 }), [0.5, 0.5]);
  assert.equal(texturePoint(400, 199, box, { width: 1024, height: 256 }), null);
  assert.deepEqual(texturePoint(400, 300, box, { width: 1024, height: 1360 }), [0.5, 0.5]);
  assert.equal(texturePoint(100, 300, box, { width: 1024, height: 1360 }), null);
});
