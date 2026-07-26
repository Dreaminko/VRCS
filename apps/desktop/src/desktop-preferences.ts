import { isTauri } from "@tauri-apps/api/core";

export interface DesktopPreferences {
  launchAtStartup: boolean;
  minimizeToTray: boolean;
}

export interface DesktopPreferencesRuntime {
  isNative: () => boolean;
  isLaunchAtStartupEnabled: () => Promise<boolean>;
  setLaunchAtStartup: (enabled: boolean) => Promise<void>;
  getMinimizeToTray: () => Promise<unknown>;
  setMinimizeToTray: (enabled: boolean) => Promise<void>;
}

export const defaultDesktopPreferences: DesktopPreferences = {
  launchAtStartup: false,
  minimizeToTray: false,
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
  const [launchAtStartup, minimizeToTray] = await Promise.all([
    adapter.isLaunchAtStartupEnabled(),
    adapter.getMinimizeToTray(),
  ]);
  return {
    launchAtStartup,
    minimizeToTray: minimizeToTray === true,
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
    if (saved !== enabled) throw new Error("Windows 开机启动状态未生效");
  } else {
    await adapter.setMinimizeToTray(enabled);
    const saved = await adapter.getMinimizeToTray();
    if ((saved === true) !== enabled) throw new Error("托盘偏好未能保存");
  }

  return { ...current, [key]: enabled };
}
