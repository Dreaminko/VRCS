import assert from "node:assert/strict";
import test from "node:test";

import { prependScrollAdjustment } from "../src/shared/lib/prepend-scroll.ts";

test("prepend scroll restoration keeps the visible subtitle at the same offset", () => {
  assert.equal(prependScrollAdjustment(24, 186, 1_000, 2_500), 162);
});

test("prepend scroll restoration does not apply a stale anchor", () => {
  assert.equal(prependScrollAdjustment(24, null, 1_000, 2_500), 0);
});

test("prepend scroll restoration falls back to the added scroll height without an anchor", () => {
  assert.equal(prependScrollAdjustment(null, null, 1_000, 2_500), 1_500);
  assert.equal(prependScrollAdjustment(null, null, 2_500, 1_000), 0);
});
