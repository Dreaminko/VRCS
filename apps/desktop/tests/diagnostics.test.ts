import assert from "node:assert/strict";
import test from "node:test";

import { normalizeFrontendError } from "../src/diagnostics.ts";

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
