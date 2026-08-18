import assert from "node:assert/strict";
import test from "node:test";

import { updaterErrorCode } from "../src/updates/app-updater.ts";

test("normalizes native updater errors to localization keys", () => {
  assert.equal(updaterErrorCode("update.no_pending"), "no_pending");
  assert.equal(updaterErrorCode(new Error("network failed")), "failed");
});
