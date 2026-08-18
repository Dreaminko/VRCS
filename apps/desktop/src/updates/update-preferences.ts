import { isTauri } from "@tauri-apps/api/core";

const STORE_KEY = "checkForUpdatesAutomatically";
const WEB_STORAGE_KEY = "vrcs.checkForUpdatesAutomatically";

export async function loadAutomaticUpdatePreference(): Promise<boolean> {
  if (!isTauri()) {
    return globalThis.localStorage?.getItem(WEB_STORAGE_KEY) !== "false";
  }
  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  return (await store.get(STORE_KEY)) !== false;
}

export async function saveAutomaticUpdatePreference(enabled: boolean): Promise<void> {
  if (!isTauri()) {
    globalThis.localStorage?.setItem(WEB_STORAGE_KEY, String(enabled));
    return;
  }
  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  await store.set(STORE_KEY, enabled);
  await store.save();
}
