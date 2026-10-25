import assert from "node:assert/strict";
import test from "node:test";
import {
  DesktopPreferenceError,
  defaultDesktopPreferences,
  loadDesktopPreferences,
  updateDesktopPreference,
} from "../src/shell/desktop-preferences.ts";
import type { DesktopPreferencesRuntime } from "../src/shell/desktop-preferences.ts";

function fakeRuntime(initial?: {
  native?: boolean;
  launchAtStartup?: boolean;
  runAsAdministrator?: unknown;
  minimizeToTray?: unknown;
}) {
  let launchAtStartup = initial?.launchAtStartup ?? false;
  let runAsAdministrator = initial?.runAsAdministrator;
  let minimizeToTray = initial?.minimizeToTray;
  const events: string[] = [];
  const runtime: DesktopPreferencesRuntime = {
    isNative: () => initial?.native ?? true,
    isLaunchAtStartupEnabled: async () => launchAtStartup,
    setLaunchAtStartup: async (enabled) => {
      events.push(`autostart:${enabled}`);
      launchAtStartup = enabled;
    },
    getRunAsAdministrator: async () => runAsAdministrator,
    setRunAsAdministrator: async (enabled) => {
      events.push(`administrator:${enabled}`);
      runAsAdministrator = enabled;
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
      runAsAdministrator: false,
      minimizeToTray: expected,
    });
  }
});

test("loads autostart from Windows and tray behavior from the preference store", async () => {
  const { runtime } = fakeRuntime({ launchAtStartup: true, minimizeToTray: true });
  assert.deepEqual(await loadDesktopPreferences(runtime), {
    launchAtStartup: true,
    runAsAdministrator: false,
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

  assert.deepEqual(tray, { launchAtStartup: true, runAsAdministrator: false, minimizeToTray: true });
  assert.deepEqual(events, ["autostart:true", "tray:true"]);
});

test("administrator launches require an explicitly enabled preference", async () => {
  for (const saved of [undefined, null, "true", 1, false, true]) {
    const { runtime } = fakeRuntime({ runAsAdministrator: saved });
    const preferences = await loadDesktopPreferences(runtime);
    assert.equal(preferences.runAsAdministrator, saved === true);
  }
});

test("enables and disables administrator launches without changing autostart or tray behavior", async () => {
  const { events, runtime } = fakeRuntime({ launchAtStartup: true, minimizeToTray: true });
  const initial = await loadDesktopPreferences(runtime);
  const enabled = await updateDesktopPreference(initial, "runAsAdministrator", true, runtime);
  assert.deepEqual(enabled, { ...initial, runAsAdministrator: true });
  assert.deepEqual(await loadDesktopPreferences(runtime), enabled);

  const disabled = await updateDesktopPreference(enabled, "runAsAdministrator", false, runtime);
  assert.deepEqual(disabled, initial);
  assert.deepEqual(await loadDesktopPreferences(runtime), initial);
  assert.deepEqual(events, ["administrator:true", "administrator:false"]);
});

test("reports administrator preference writes that did not persist", async () => {
  for (const enabled of [true, false]) {
    const { runtime } = fakeRuntime({ runAsAdministrator: !enabled });
    runtime.setRunAsAdministrator = async () => {};
    await assert.rejects(
      updateDesktopPreference(defaultDesktopPreferences, "runAsAdministrator", enabled, runtime),
      (error: unknown) => error instanceof DesktopPreferenceError
        && error.code === "desktop.administrator_not_saved",
    );
  }
});
