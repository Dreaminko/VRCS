import assert from "node:assert/strict";
import test from "node:test";

import {
  MAX_RECENT_REPORTS,
  normalizeFrontendError,
  RecentReportTracker,
} from "../src/diagnostics.ts";

test("normalizes Error instances for frontend reporting", () => {
  const error = new Error("render failed");
  const normalized = normalizeFrontendError(error);

  assert.equal(normalized.message, "render failed");
  assert.match(normalized.stack ?? "", /render failed/);
});

test("does not serialize arbitrary rejected objects", () => {
  assert.deepEqual(
    normalizeFrontendError({ token: "must-not-be-logged" }),
    { message: "Unknown frontend error" },
  );
});

test("recent frontend reports are deduplicated until they expire", () => {
  const reports = new RecentReportTracker();

  assert.equal(reports.accept("render:app:failed", 1_000), true);
  assert.equal(reports.accept("render:app:failed", 2_999), false);
  assert.equal(reports.accept("render:app:failed", 3_000), true);
});

test("recent frontend reports stay within their hard limit", () => {
  const reports = new RecentReportTracker(Number.POSITIVE_INFINITY);

  for (let index = 0; index < MAX_RECENT_REPORTS + 10; index += 1) {
    assert.equal(reports.accept(`error-${index}`, index), true);
  }

  assert.equal(reports.size, MAX_RECENT_REPORTS);
});
