import assert from "node:assert/strict";
import test from "node:test";

import {
  applyInterfaceScale,
  DEFAULT_INTERFACE_SCALE,
  INTERFACE_LAYOUT_CHANGE_EVENT,
  interfaceScaleFactor,
  interfaceScaleShortcutStep,
  normalizeInterfaceScale,
} from "../src/app/interface-scale.ts";

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

test("computes normalized interface scale factor", () => {
  assert.equal(interfaceScaleFactor(150), 1.5);
  assert.equal(interfaceScaleFactor(75), 0.75);
  assert.equal(interfaceScaleFactor(null), 1);
});

test("applies native zoom and ignores stale out-of-order requests", async () => {
  const dispatchedEvents: string[] = [];
  const applied: number[] = [];
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      dispatchEvent: (event: Event) => dispatchedEvents.push(event.type),
    },
  });

  try {
    await applyInterfaceScale(150, async (scale) => {
      applied.push(scale);
    });
    assert.deepEqual(applied, [1.5]);
    assert.deepEqual(dispatchedEvents, [INTERFACE_LAYOUT_CHANGE_EVENT]);

    let resolveStale!: () => void;
    const stale = applyInterfaceScale(75, () => new Promise<void>((resolve) => {
      resolveStale = resolve;
    }));
    await applyInterfaceScale(100, async (scale) => {
      applied.push(scale);
    });
    resolveStale();
    await stale;

    assert.deepEqual(applied, [1.5, 1]);
    assert.deepEqual(dispatchedEvents, [INTERFACE_LAYOUT_CHANGE_EVENT, INTERFACE_LAYOUT_CHANGE_EVENT]);
  } finally {
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else delete (globalThis as { window?: unknown }).window;
  }
});
