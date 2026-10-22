import assert from "node:assert/strict";
import test from "node:test";
import { layoutOcrScene, quadTransform, type OcrScene, type Quad } from "../src/vr-surface/ocr-scene.ts";

const quad = (left: number, top: number, right: number, bottom: number): Quad => [[left, top], [right, top], [right, bottom], [left, bottom]];
const measure = (text: string, size: number) => Array.from(text).length * size * 0.55;
const scene = (blocks: OcrScene["blocks"], excluded: Quad[] = []): OcrScene => ({ blocks, excluded, opacity: 0.85, aspect: 1 });

test("OCR cards preserve text and source quadrilateral", () => {
  const source = quad(0.1, 0.1, 0.4, 0.25);
  const result = layoutOcrScene(scene([{ id: 7, text: "Hello 世界", quad: source, masks: [source] }]), measure);
  assert.deepEqual(result.unplaced, []);
  assert.equal(result.cards[0]?.id, 7);
  assert.equal(result.cards[0]?.connected, false);
  assert.equal(result.cards[0]?.lines.join(""), "Hello 世界");
});

test("OCR text overflow reports the original group instead of clipping", () => {
  const source = quad(0.45, 0.45, 0.5, 0.46);
  const result = layoutOcrScene(scene([{ id: 19, text: "translation ".repeat(10000), quad: source, masks: [source] }]), measure);
  assert.deepEqual(result.unplaced, [19]);
  assert.equal(result.cards.length, 0);
});

test("OCR text can expand and move beside a source near the edge", () => {
  const source = quad(0.01, 0.4, 0.035, 0.415);
  const result = layoutOcrScene(scene([{ id: 4, text: "A longer translation with enough room beside the source", quad: source, masks: [source] }]), measure);
  assert.deepEqual(result.unplaced, []);
  assert.equal(result.cards[0]?.connected, true);
  assert.ok(result.cards[0]!.quad[0][0] > source[1][0] * 2048);
});

test("OCR overflow does not cover an unrelated source block", () => {
  const source = quad(0.4, 0.4, 0.43, 0.42);
  const blockers = [quad(0.03, 0.03, 0.4, 0.97), quad(0.43, 0.03, 0.97, 0.97), quad(0.4, 0.03, 0.43, 0.4), quad(0.4, 0.42, 0.43, 0.97)];
  const result = layoutOcrScene(scene([{ id: 8, text: "A translation that needs more room", quad: source, masks: [source] }], blockers), measure);
  assert.deepEqual(result.unplaced, [8]);
});

test("OCR projection maps every corner through a perspective transform", () => {
  const target: Quad = [[30, 40], [500, 80], [450, 360], [10, 300]];
  const matrix = quadTransform(target, 600, 400);
  assert.ok(matrix);
  for (const [[x, y], expected] of ([[[0, 0], target[0]], [[600, 0], target[1]], [[600, 400], target[2]], [[0, 400], target[3]]] as const)) {
    const w = matrix[3]! * x + matrix[7]! * y + matrix[15]!;
    assert.ok(Math.abs((matrix[0]! * x + matrix[4]! * y + matrix[12]!) / w - expected[0]) < 0.0001);
    assert.ok(Math.abs((matrix[1]! * x + matrix[5]! * y + matrix[13]!) / w - expected[1]) < 0.0001);
  }
});

test("OCR scene uses physical aspect when laying out text", () => {
  const source = quad(0.1, 0.1, 0.8, 0.5);
  const result = layoutOcrScene({ ...scene([{ id: 1, text: "Readable", quad: source, masks: [source] }]), aspect: 2 }, measure);
  assert.equal(result.height, 1024);
  assert.equal(result.cards[0]?.height, 409.6);
});
