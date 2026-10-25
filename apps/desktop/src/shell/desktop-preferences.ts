import { isTauri } from "@tauri-apps/api/core";

export interface DesktopPreferences {
  launchAtStartup: boolean;
  runAsAdministrator: boolean;
  minimizeToTray: boolean;
}

export class DesktopPreferenceError extends Error {
  readonly code: string;

  constructor(code: string) {
    super(code);
    this.name = "DesktopPreferenceError";
    this.code = code;
  }
}

export interface DesktopPreferencesRuntime {
  isNative: () => boolean;
  isLaunchAtStartupEnabled: () => Promise<boolean>;
  setLaunchAtStartup: (enabled: boolean) => Promise<void>;
  getRunAsAdministrator: () => Promise<unknown>;
  setRunAsAdministrator: (enabled: boolean) => Promise<void>;
  getMinimizeToTray: () => Promise<unknown>;
  setMinimizeToTray: (enabled: boolean) => Promise<void>;
}

export const defaultDesktopPreferences: DesktopPreferences = {
  launchAtStartup: false,
  runAsAdministrator: false,
  minimizeToTray: true,
};

const runtime: DesktopPreferencesRuntime = {
  isNative: isTauri,
  isLaunchAtStartupEnabled: async () => {
    const { isEnabled } = await import("@tauri-apps/plugin-autostart");
    return isEnabled();
  },
  setLaunchAtStartup: async (enabled) => {
    const { disable, enable } = await import("@tauri-apps/plugin-autostart");
    if (enabled) await enable();
    else await disable();
  },
  getRunAsAdministrator: async () => {
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("preferences.json", { autoSave: false });
    return store.get("runAsAdministrator");
  },
  setRunAsAdministrator: async (enabled) => {
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("preferences.json", { autoSave: false });
    await store.set("runAsAdministrator", enabled);
    await store.save();
  },
  getMinimizeToTray: async () => {
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("preferences.json", { autoSave: false });
    return store.get("minimizeToTray");
  },
  setMinimizeToTray: async (enabled) => {
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("preferences.json", { autoSave: false });
    await store.set("minimizeToTray", enabled);
    await store.save();
  },
};

export async function loadDesktopPreferences(
  adapter: DesktopPreferencesRuntime = runtime,
): Promise<DesktopPreferences> {
  if (!adapter.isNative()) return defaultDesktopPreferences;
  const [launchAtStartup, runAsAdministrator, minimizeToTray] = await Promise.all([
    adapter.isLaunchAtStartupEnabled(),
    adapter.getRunAsAdministrator(),
    adapter.getMinimizeToTray(),
  ]);
  return {
    launchAtStartup,
    runAsAdministrator: runAsAdministrator === true,
    minimizeToTray: typeof minimizeToTray === "boolean"
      ? minimizeToTray
      : defaultDesktopPreferences.minimizeToTray,
  };
}

export async function updateDesktopPreference(
  current: DesktopPreferences,
  key: keyof DesktopPreferences,
  enabled: boolean,
  adapter: DesktopPreferencesRuntime = runtime,
): Promise<DesktopPreferences> {
  if (!adapter.isNative()) return { ...current, [key]: enabled };

  if (key === "launchAtStartup") {
    await adapter.setLaunchAtStartup(enabled);
    const saved = await adapter.isLaunchAtStartupEnabled();
    if (saved !== enabled) {
      throw new DesktopPreferenceError("desktop.autostart_not_applied");
    }
  } else if (key === "runAsAdministrator") {
    await adapter.setRunAsAdministrator(enabled);
    const saved = await adapter.getRunAsAdministrator();
    if (saved !== enabled) {
      throw new DesktopPreferenceError("desktop.administrator_not_saved");
    }
  } else {
    await adapter.setMinimizeToTray(enabled);
    const saved = await adapter.getMinimizeToTray();
    if ((saved === true) !== enabled) {
      throw new DesktopPreferenceError("desktop.tray_not_saved");
    }
  }

  return { ...current, [key]: enabled };
}
