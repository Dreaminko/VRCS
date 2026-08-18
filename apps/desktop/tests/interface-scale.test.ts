import assert from "node:assert/strict";
import test from "node:test";

import {
  applyInterfaceScale,
  COMPACT_OVERLAY_HEIGHT,
  DEFAULT_INTERFACE_SCALE,
  interfaceLayoutPixels,
  interfaceScaleFactors,
  interfaceScaleShortcutStep,
  interfaceViewportMetrics,
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

test("keeps layout coordinates stable while scaling interface lengths", () => {
  assert.deepEqual(interfaceScaleFactors(150), {
    scale: 1.5,
    inverse: 2 / 3,
  });
  assert.equal(interfaceLayoutPixels(1180, 1.5), 1180 / 1.5);
  assert.equal(interfaceLayoutPixels(760, 0.75), 760 / 0.75);
});

test("derives scale-aware overlay viewport metrics", () => {
  assert.deepEqual(interfaceViewportMetrics(1180, 760, 1), {
    width: 1180,
    height: 760,
    overlayGutter: 24,
  });
  assert.deepEqual(interfaceViewportMetrics(1180, 760, 1.5), {
    width: 1180 / 1.5,
    height: 760 / 1.5,
    overlayGutter: 12,
  });
  assert.equal(
    interfaceViewportMetrics(860, COMPACT_OVERLAY_HEIGHT, 1).overlayGutter,
    24,
  );
});

test("applies interface scale through CSS factors", async () => {
  const properties = new Map<string, string>();
  const dispatchedEvents: string[] = [];
  const previousDocument = Object.getOwnPropertyDescriptor(globalThis, "document");
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: {
      documentElement: {
        style: {
          getPropertyValue: (name: string) => properties.get(name) ?? "",
          setProperty: (name: string, value: string) => properties.set(name, value),
        },
      },
    },
  });
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      innerWidth: 1180,
      innerHeight: 760,
      dispatchEvent: (event: Event) => dispatchedEvents.push(event.type),
    },
  });

  try {
    await applyInterfaceScale(150);
  } finally {
    if (previousDocument) Object.defineProperty(globalThis, "document", previousDocument);
    else delete (globalThis as { document?: unknown }).document;
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else delete (globalThis as { window?: unknown }).window;
  }

  assert.equal(properties.get("--interface-scale"), "1.5");
  assert.equal(Number(properties.get("--interface-scale-inverse")), 2 / 3);
  assert.equal(properties.get("--interface-layout-width"), `${1180 / 1.5}px`);
  assert.equal(properties.get("--interface-layout-height"), `${760 / 1.5}px`);
  assert.equal(properties.get("--interface-overlay-gutter"), "12px");
  assert.deepEqual(dispatchedEvents, ["vrcs:interface-layout-change"]);
});
