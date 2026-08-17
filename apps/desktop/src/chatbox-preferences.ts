import { isTauri } from "@tauri-apps/api/core";

import {
  chatboxPreferencesFromDraft,
  normalizeChatboxPreferences,
} from "./chatbox";
import type { ChatboxPreferences } from "./chatbox";
import { LatestWriteQueue } from "./latest-write-queue";

const STORE_KEY = "chatboxSendSettings";
const WEB_STORAGE_KEY = "vrcs.chatbox-send-settings.v1";

async function persistChatboxPreferences(preferences: ChatboxPreferences): Promise<void> {
  try {
    if (isTauri()) {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("preferences.json", { autoSave: false });
      await store.set(STORE_KEY, preferences);
      await store.save();
      return;
    }
  } catch {
    // Keep the browser-backed copy below as a fallback if the native store fails.
  }
  try {
    globalThis.localStorage?.setItem(WEB_STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    // Persistence is best-effort when neither storage backend is writable.
  }
}

const writeQueue = new LatestWriteQueue(persistChatboxPreferences);

export function chatboxPreferencesSnapshot(
  fallbackTarget: ChatboxPreferences["target_language"] = "ja",
): ChatboxPreferences {
  try {
    return normalizeChatboxPreferences(
      globalThis.localStorage?.getItem(WEB_STORAGE_KEY),
      fallbackTarget,
    );
  } catch {
    return normalizeChatboxPreferences(null, fallbackTarget);
  }
}

export async function loadChatboxPreferences(
  fallbackTarget: ChatboxPreferences["target_language"] = "ja",
): Promise<ChatboxPreferences> {
  const fallback = chatboxPreferencesSnapshot(fallbackTarget);
  try {
    if (!isTauri()) return fallback;
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load("preferences.json", { autoSave: false });
    const stored = await store.get<unknown>(STORE_KEY);
    return stored == null ? fallback : normalizeChatboxPreferences(stored, fallbackTarget);
  } catch {
    return fallback;
  }
}

export function saveChatboxPreferences(preferences: ChatboxPreferences): Promise<void> {
  return writeQueue.enqueue(
    normalizeChatboxPreferences(preferences, preferences.target_language),
  );
}
