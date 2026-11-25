import { invoke, isTauri } from "@tauri-apps/api/core";

const AUTOMATIC_CHECKS_KEY = "checkForUpdatesAutomatically";
const NIGHTLY_UPDATES_KEY = "receiveNightlyUpdates";
const SKIPPED_VERSION_KEY = "skippedUpdateVersion";
const LAST_CHECKED_AT_KEY = "lastUpdateCheckAt";

export interface UpdatePreferences {
  automaticChecks: boolean;
  receiveNightlyUpdates: boolean;
  skippedVersion: string | null;
  lastCheckedAt: number | null;
}

export async function loadUpdatePreferences(): Promise<UpdatePreferences> {
  if (!isTauri()) {
    const skipped = globalThis.localStorage?.getItem(`vrcs.${SKIPPED_VERSION_KEY}`)?.trim() || null;
    const rawCheckedAt = Number(globalThis.localStorage?.getItem(`vrcs.${LAST_CHECKED_AT_KEY}`));
    return {
      automaticChecks: globalThis.localStorage?.getItem(`vrcs.${AUTOMATIC_CHECKS_KEY}`) !== "false",
      receiveNightlyUpdates: globalThis.localStorage?.getItem(`vrcs.${NIGHTLY_UPDATES_KEY}`) === "true",
      skippedVersion: skipped,
      lastCheckedAt: Number.isFinite(rawCheckedAt) && rawCheckedAt > 0 ? rawCheckedAt : null,
    };
  }
  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  const rawSkipped = await store.get(SKIPPED_VERSION_KEY);
  const rawCheckedAt = await store.get(LAST_CHECKED_AT_KEY);
  return {
    automaticChecks: (await store.get(AUTOMATIC_CHECKS_KEY)) !== false,
    receiveNightlyUpdates: (await store.get(NIGHTLY_UPDATES_KEY)) === true,
    skippedVersion: typeof rawSkipped === "string" && rawSkipped.trim() ? rawSkipped.trim() : null,
    lastCheckedAt: typeof rawCheckedAt === "number" && Number.isFinite(rawCheckedAt) && rawCheckedAt > 0
      ? rawCheckedAt
      : null,
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

export async function saveSkippedUpdateVersion(version: string | null): Promise<void> {
  const normalized = version?.trim() || null;
  if (!isTauri()) {
    if (normalized) {
      globalThis.localStorage?.setItem(`vrcs.${SKIPPED_VERSION_KEY}`, normalized);
    } else {
      globalThis.localStorage?.removeItem(`vrcs.${SKIPPED_VERSION_KEY}`);
    }
    return;
  }
  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  if (normalized) {
    await store.set(SKIPPED_VERSION_KEY, normalized);
  } else {
    await store.delete(SKIPPED_VERSION_KEY);
  }
  await store.save();
}

export async function saveLastUpdateCheckAt(timestamp: number): Promise<void> {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return;
  if (!isTauri()) {
    globalThis.localStorage?.setItem(`vrcs.${LAST_CHECKED_AT_KEY}`, String(timestamp));
    return;
  }
  const { load } = await import("@tauri-apps/plugin-store");
  const store = await load("preferences.json", { autoSave: false });
  await store.set(LAST_CHECKED_AT_KEY, timestamp);
  await store.save();
}
