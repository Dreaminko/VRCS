import assert from "node:assert/strict";
import test from "node:test";
import { placeLookupPopover } from "../src/popover-placement.ts";

test("places the popover below when there is enough space", () => {
  const result = placeLookupPopover({
    anchor: { top: 100, bottom: 120, centerX: 200 },
    popoverHeight: 200,
    viewportHeight: 600,
  });

  assert.equal(result.side, "below");
  assert.equal(result.top, 130);
});

test("places the popover above when the lower space is insufficient", () => {
  const result = placeLookupPopover({
    anchor: { top: 500, bottom: 520, centerX: 200 },
    popoverHeight: 200,
    viewportHeight: 600,
  });

  assert.equal(result.side, "above");
  assert.equal(result.top, 290);
});

test("constrains the popover without overlapping the selection", () => {
  const anchor = { top: 214, bottom: 235, centerX: 86 };
  const popoverHeight = 218;
  const result = placeLookupPopover({ anchor, popoverHeight, viewportHeight: 400 });
  const visibleHeight = Math.min(popoverHeight, result.maxHeight);

  assert.equal(result.side, "above");
  assert.equal(result.top + visibleHeight, anchor.top - 10);
});
