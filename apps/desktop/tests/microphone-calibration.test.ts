import assert from "node:assert/strict";
import test from "node:test";

import { suggestMicrophoneThreshold } from "../src/microphone-calibration.ts";

test("suggests a threshold between background noise and normal speech", () => {
  const result = suggestMicrophoneThreshold(
    [-62, -61, -60, -59, -58, -57],
    [-39, -38, -37, -36, -35, -34],
  );
  assert.ok(result);
  assert.ok(result.threshold > result.noiseLevel);
  assert.ok(result.threshold < result.speechLevel);
});

test("rejects calibration when speech is not distinct from noise", () => {
  assert.equal(suggestMicrophoneThreshold(
    [-47, -46, -45, -44],
    [-43, -42, -41, -40],
  ), null);
});

test("rejects empty or non-finite samples", () => {
  assert.equal(suggestMicrophoneThreshold([], [-35]), null);
  assert.equal(suggestMicrophoneThreshold([Number.NaN], [Number.POSITIVE_INFINITY]), null);
});
