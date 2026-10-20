import assert from "node:assert/strict";
import test from "node:test";
import {
  defaultDesktopPreferences,
  loadDesktopPreferences,
  updateDesktopPreference,
} from "../src/shell/desktop-preferences.ts";
import type { DesktopPreferencesRuntime } from "../src/shell/desktop-preferences.ts";

function fakeRuntime(initial?: {
  native?: boolean;
  launchAtStartup?: boolean;
  minimizeToTray?: unknown;
}) {
  let launchAtStartup = initial?.launchAtStartup ?? false;
  let minimizeToTray = initial?.minimizeToTray;
  const events: string[] = [];
  const runtime: DesktopPreferencesRuntime = {
    isNative: () => initial?.native ?? true,
    isLaunchAtStartupEnabled: async () => launchAtStartup,
    setLaunchAtStartup: async (enabled) => {
      events.push(`autostart:${enabled}`);
      launchAtStartup = enabled;
    },
    getMinimizeToTray: async () => minimizeToTray,
    setMinimizeToTray: async (enabled) => {
      events.push(`tray:${enabled}`);
      minimizeToTray = enabled;
    },
  };
  return { events, runtime };
}

test("uses desktop defaults outside the native app", async () => {
  const { runtime } = fakeRuntime({ native: false, launchAtStartup: true, minimizeToTray: true });
  assert.deepEqual(await loadDesktopPreferences(runtime), defaultDesktopPreferences);
});

test("missing tray preferences default to enabled while saved choices are preserved", async () => {
  for (const [saved, expected] of [[undefined, true], [null, true], [true, true], [false, false]] as const) {
    const { runtime } = fakeRuntime({ minimizeToTray: saved });
    assert.deepEqual(await loadDesktopPreferences(runtime), {
      launchAtStartup: false,
      minimizeToTray: expected,
    });
  }
});

test("loads autostart from Windows and tray behavior from the preference store", async () => {
  const { runtime } = fakeRuntime({ launchAtStartup: true, minimizeToTray: true });
  assert.deepEqual(await loadDesktopPreferences(runtime), {
    launchAtStartup: true,
    minimizeToTray: true,
  });
});

test("updates and verifies each desktop preference independently", async () => {
  const { events, runtime } = fakeRuntime();
  const startup = await updateDesktopPreference(
    defaultDesktopPreferences,
    "launchAtStartup",
    true,
    runtime,
  );
  const tray = await updateDesktopPreference(startup, "minimizeToTray", true, runtime);

  assert.deepEqual(tray, { launchAtStartup: true, minimizeToTray: true });
  assert.deepEqual(events, ["autostart:true", "tray:true"]);
});
