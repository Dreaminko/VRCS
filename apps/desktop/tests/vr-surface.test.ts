import assert from "node:assert/strict";
import test from "node:test";
import { markerBits, menuStatus, newerKey, pageGeometry, sameKey } from "../src/vr-surface/protocol.ts";

test("menu distinguishes requested settings from effective content and prioritizes failures and busy", () => {
  const item = { requested: true, effective: false, available: true, busy: false, reason: null, label: "Wrist dialogue" };
  const labels = { waitingContent: "Waiting for content", controlBusy: "Working" };
  assert.deepEqual(menuStatus(item, labels), { status: "waiting", label: "Wrist dialogue · Waiting for content" });
  assert.deepEqual(menuStatus({ ...item, effective: true }, labels), { status: "idle", label: item.label });
  assert.equal(menuStatus({ ...item, requested: false }, labels).status, "idle");
  assert.deepEqual(menuStatus({ ...item, reason: "Overlay disabled" }, labels), { status: "unavailable", label: "Wrist dialogue · Overlay disabled" });
  assert.deepEqual(menuStatus({ ...item, busy: true, reason: "Old error" }, labels), { status: "busy", label: "Wrist dialogue · Working" });
});

test("surface identity rejects stale generations, revisions and invalid 32-bit keys", () => {
  const current = { generation: 7, revision: 22 };
  assert.equal(newerKey({ generation: 8, revision: 0 }, current), true);
  assert.equal(newerKey({ generation: 7, revision: 23 }, current), true);
  assert.equal(newerKey({ generation: 6, revision: 999 }, current), false);
  assert.equal(newerKey(current, current), false);
  assert.equal(sameKey({ ...current }, current), true);
  assert.equal(sameKey(null, current), false);
  for (const value of [-1, 0.5, Number.NaN, Number.POSITIVE_INFINITY, 0x100000000]) {
    assert.equal(newerKey({ generation: value, revision: 1 }, null), false);
  }
});

test("capture marker contains generation then revision as 32 MSB-first bits", () => {
  const bits = markerBits({ generation: 0x80000001, revision: 0x12345678 });
  assert.equal(bits.length, 64);
  assert.deepEqual(bits.slice(0, 32), [1, ...Array(30).fill(0), 1]);
  const decode = (value: number[]) => value.reduce((word, bit) => word * 2 + bit, 0);
  assert.equal(decode(bits.slice(0, 32)), 0x80000001);
  assert.equal(decode(bits.slice(32)), 0x12345678);
});

test("reader pages overlap so every vertical position remains reachable", () => {
  const height = 10327;
  const viewport = 640;
  const first = pageGeometry(height, viewport, 0, 48);
  let previousEnd = 0;
  for (let page = 0; page < first.count; page++) {
    const geometry = pageGeometry(height, viewport, page, 48);
    assert.ok(geometry.offset <= previousEnd);
    if (page) assert.ok(geometry.offset < previousEnd, "a line cut by a viewport edge remains visible on the adjacent page");
    previousEnd = geometry.offset + viewport;
  }
  assert.equal(previousEnd, height);
});

test("reader paging clamps obsolete page indexes and small or invalid measurements", () => {
  assert.deepEqual(pageGeometry(100, 600, 9, 32), { count: 1, page: 0, offset: 0 });
  assert.deepEqual(pageGeometry(1000, 600, 99, 32), { count: 2, page: 1, offset: 400 });
  assert.deepEqual(pageGeometry(1000, 600, -3, 32), { count: 2, page: 0, offset: 0 });
  assert.deepEqual(pageGeometry(1000, 0, 0, 32), { count: 1, page: 0, offset: 0 });
  assert.deepEqual(pageGeometry(Number.NaN, 600, 0, 32), { count: 1, page: 0, offset: 0 });
});
