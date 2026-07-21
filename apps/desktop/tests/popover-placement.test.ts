import assert from "node:assert/strict";
import test from "node:test";
import {
  isLookupAnchorVisible,
  LOOKUP_POPOVER_HEIGHT,
  placeLookupPopover,
} from "../src/popover-placement.ts";

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

test("uses a stable popover height when the viewport has enough space", () => {
  const result = placeLookupPopover({
    anchor: { top: 80, bottom: 100, centerX: 200 },
    popoverHeight: LOOKUP_POPOVER_HEIGHT,
    viewportHeight: 800,
  });

  assert.equal(result.height, LOOKUP_POPOVER_HEIGHT);
});

test("shrinks the fixed popover only when the viewport space is limited", () => {
  const result = placeLookupPopover({
    anchor: { top: 220, bottom: 240, centerX: 200 },
    popoverHeight: LOOKUP_POPOVER_HEIGHT,
    viewportHeight: 420,
  });

  assert.equal(result.height, result.maxHeight);
  assert.ok(result.height < LOOKUP_POPOVER_HEIGHT);
});

test("keeps a partially visible lookup anchor open", () => {
  assert.equal(isLookupAnchorVisible(
    { top: 35, bottom: 55, left: 100, right: 150, width: 50, height: 20 },
    800,
    600,
    40,
  ), true);
});

test("hides the lookup when its anchor leaves the viewport", () => {
  const above = { top: 10, bottom: 40, left: 100, right: 150, width: 50, height: 30 };
  const below = { top: 600, bottom: 620, left: 100, right: 150, width: 50, height: 20 };
  const left = { top: 100, bottom: 120, left: -60, right: 0, width: 60, height: 20 };

  assert.equal(isLookupAnchorVisible(above, 800, 600, 40), false);
  assert.equal(isLookupAnchorVisible(below, 800, 600, 40), false);
  assert.equal(isLookupAnchorVisible(left, 800, 600, 40), false);
});
