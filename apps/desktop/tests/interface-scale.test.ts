import assert from "node:assert/strict";
import test from "node:test";

import {
  applyInterfaceScale,
  DEFAULT_INTERFACE_SCALE,
  interfaceLayoutPixels,
  interfaceScaleFactors,
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

test("keeps layout coordinates stable while scaling interface lengths", () => {
  assert.deepEqual(interfaceScaleFactors(150), {
    scale: 1.5,
    inverse: 2 / 3,
  });
  assert.equal(interfaceLayoutPixels(1180, 1.5), 1180 / 1.5);
  assert.equal(interfaceLayoutPixels(760, 0.75), 760 / 0.75);
});

test("applies interface scale through CSS factors", async () => {
  const properties = new Map<string, string>();
  const previousDocument = Object.getOwnPropertyDescriptor(globalThis, "document");
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: {
      documentElement: {
        style: {
          setProperty: (name: string, value: string) => properties.set(name, value),
        },
      },
    },
  });

  try {
    await applyInterfaceScale(150);
  } finally {
    if (previousDocument) Object.defineProperty(globalThis, "document", previousDocument);
    else delete (globalThis as { document?: unknown }).document;
  }

  assert.equal(properties.get("--interface-scale"), "1.5");
  assert.equal(Number(properties.get("--interface-scale-inverse")), 2 / 3);
});
