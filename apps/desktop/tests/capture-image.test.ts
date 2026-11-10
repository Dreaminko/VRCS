import assert from "node:assert/strict";
import test from "node:test";
import { imageRegion } from "../src/capture-image/geometry.ts";

test("reverse fractional bitmap selection clamps without desktop scale assumptions", () => {
  assert.deepEqual(imageRegion({ x: 4000.2, y: 2040.4 }, { x: -12.5, y: 20.2 }, 3840, 2160), {
    left: 0, top: 20, width: 3840, height: 2021,
  });
  assert.deepEqual(imageRegion({ x: 20.4, y: 30.4 }, { x: 40.1, y: 60.1 }, 100, 100), {
    left: 20, top: 30, width: 21, height: 31,
  });
});

test("clicks, off-image drags and non-finite coordinates cannot select a region", () => {
  assert.equal(imageRegion({ x: 5, y: 5 }, { x: 5, y: 5 }, 100, 100), null);
  assert.equal(imageRegion({ x: -20, y: -20 }, { x: -2, y: -2 }, 100, 100), null);
  assert.equal(imageRegion({ x: NaN, y: 5 }, { x: 50, y: 50 }, 100, 100), null);
});
