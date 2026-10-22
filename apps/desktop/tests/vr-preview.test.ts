import assert from "node:assert/strict";
import test from "node:test";
import { decodeFrame, texturePoint } from "../src/vr-preview/frame.ts";
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

function frame(format = 1, alpha = 1) {
  const buffer = new ArrayBuffer(44);
  const view = new DataView(buffer);
  [0x56524353, 1, 1, 1, format, alpha, 2, 3, 4, 5].forEach((v, i) => view.setUint32(i * 4, v, true));
  new Uint8Array(buffer, 40).set([20, 40, 60, 128]);
  return buffer;
}

test("preview decodes frame identity and straight RGBA without changing pixels", () => {
  const decoded = decodeFrame(frame());
  assert.deepEqual([decoded.width, decoded.height, decoded.generation, decoded.revision, decoded.panel, decoded.sequence], [1, 1, 2, 3, 4, 5]);
  assert.deepEqual([...decoded.pixels], [20, 40, 60, 128]);
});

test("preview converts premultiplied BGRA for Canvas and rejects malformed frames", () => {
  assert.deepEqual([...decodeFrame(frame(2, 2)).pixels], [120, 80, 40, 128]);
  assert.throws(() => decodeFrame(frame().slice(0, 42)));
  assert.throws(() => decodeFrame(frame(3)));
  const broken = frame();
  new DataView(broken).setUint32(8, 0xffffffff, true);
  assert.throws(() => decodeFrame(broken));
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
