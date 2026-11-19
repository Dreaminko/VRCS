import { invoke, isTauri } from "@tauri-apps/api/core";

const AUTOMATIC_CHECKS_KEY = "checkForUpdatesAutomatically";
const NIGHTLY_UPDATES_KEY = "receiveNightlyUpdates";

export async function loadUpdatePreferences() {
  if (!isTauri()) {
    return {
      automaticChecks: globalThis.localStorage?.getItem(`vrcs.${AUTOMATIC_CHECKS_KEY}`) !== "false",
      receiveNightlyUpdates: globalThis.localStorage?.getItem(`vrcs.${NIGHTLY_UPDATES_KEY}`) === "true",
    };
  }
  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  return {
    automaticChecks: (await store.get(AUTOMATIC_CHECKS_KEY)) !== false,
    receiveNightlyUpdates: (await store.get(NIGHTLY_UPDATES_KEY)) === true,
  };
}

export async function saveAutomaticUpdatePreference(enabled: boolean): Promise<void> {
  if (!isTauri()) {
    globalThis.localStorage?.setItem(`vrcs.${AUTOMATIC_CHECKS_KEY}`, String(enabled));
    return;
  }
  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  await store.set(AUTOMATIC_CHECKS_KEY, enabled);
  await store.save();
}

export async function saveNightlyUpdatePreference(enabled: boolean): Promise<void> {
  if (!isTauri()) {
    globalThis.localStorage?.setItem(`vrcs.${NIGHTLY_UPDATES_KEY}`, String(enabled));
    return;
  }
  await invoke("set_receive_nightly_updates", { enabled });
}
