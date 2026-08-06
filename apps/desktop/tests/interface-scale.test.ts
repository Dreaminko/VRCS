import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_INTERFACE_SCALE,
  interfaceScaleShortcutStep,
  normalizeInterfaceScale,
} from "../src/interface-scale.ts";

test("normalizes interface scale to 5 percent steps within its supported range", () => {
  assert.equal(normalizeInterfaceScale(73), 75);
  assert.equal(normalizeInterfaceScale(112), 110);
  assert.equal(normalizeInterfaceScale(148), 150);
  assert.equal(normalizeInterfaceScale(999), 150);
  assert.equal(normalizeInterfaceScale(null), DEFAULT_INTERFACE_SCALE);
  assert.equal(normalizeInterfaceScale("not-a-number"), DEFAULT_INTERFACE_SCALE);
});

test("recognizes Ctrl plus and minus on the main keyboard and numpad", () => {
  const shortcut = (
    key: string,
    code: string,
    overrides: Partial<Pick<KeyboardEvent, "altKey" | "ctrlKey" | "metaKey">> = {},
  ) => interfaceScaleShortcutStep({
    altKey: false,
    ctrlKey: true,
    metaKey: false,
    key,
    code,
    ...overrides,
  });

  assert.equal(shortcut("=", "Equal"), 5);
  assert.equal(shortcut("+", "Equal"), 5);
  assert.equal(shortcut("+", "NumpadAdd"), 5);
  assert.equal(shortcut("-", "Minus"), -5);
  assert.equal(shortcut("-", "NumpadSubtract"), -5);
  assert.equal(shortcut("=", "Equal", { ctrlKey: false }), 0);
  assert.equal(shortcut("=", "Equal", { altKey: true }), 0);
});
