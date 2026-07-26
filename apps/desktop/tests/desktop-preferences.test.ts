import assert from "node:assert/strict";
import test from "node:test";
import {
  defaultDesktopPreferences,
  loadDesktopPreferences,
  updateDesktopPreference,
} from "../src/desktop-preferences.ts";
import type { DesktopPreferencesRuntime } from "../src/desktop-preferences.ts";

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

test("uses quiet defaults outside the native app", async () => {
  const { runtime } = fakeRuntime({ native: false, launchAtStartup: true, minimizeToTray: true });
  assert.deepEqual(await loadDesktopPreferences(runtime), defaultDesktopPreferences);
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
