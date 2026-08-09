import { isTauri } from "@tauri-apps/api/core";

import type { ConversationCustomization, ConversationIcon } from "./conversations";

const STORE_FILE = "conversations.json";
const STORE_KEY = "state";
const FALLBACK_KEY = "vrcs.conversation-state.v2";
const LEGACY_STARTS_KEY = "vrcs.conversation-starts.v1";
const MAX_CONVERSATIONS = 50;
const VALID_ICONS = new Set<ConversationIcon>([
  "message",
  "game",
  "headphones",
  "languages",
  "study",
  "users",
  "bookmark",
  "sparkles",
  "mic",
  "music",
  "video",
  "globe",
  "heart",
  "star",
  "coffee",
  "trophy",
]);

export interface ConversationState {
  starts: number[];
  customizations: Record<string, ConversationCustomization>;
}

const EMPTY_STATE: ConversationState = { starts: [], customizations: {} };

export function normalizeConversationTitle(value: string) {
  return Array.from(value.replace(/\s+/g, " ").trim()).slice(0, 40).join("");
}

function normalizeStarts(value: unknown) {
  if (!Array.isArray(value)) return [];
  return [...new Set(value.filter(
    (item): item is number => typeof item === "number" && Number.isFinite(item),
  ))]
    .sort((left, right) => left - right)
    .slice(-MAX_CONVERSATIONS);
}

function isConversationIcon(value: unknown): value is ConversationIcon {
  return typeof value === "string" && VALID_ICONS.has(value as ConversationIcon);
}

export function normalizeConversationState(value: unknown): ConversationState {
  if (!value || typeof value !== "object") return EMPTY_STATE;
  const candidate = value as {
    starts?: unknown;
    customizations?: unknown;
  };
  const customizations: Record<string, ConversationCustomization> = {};

  if (candidate.customizations && typeof candidate.customizations === "object") {
    Object.entries(candidate.customizations).forEach(([id, raw]) => {
      if (!/^conversation-\d+$/.test(id) || !raw || typeof raw !== "object") return;
      const entry = raw as { title?: unknown; icon?: unknown };
      const title = typeof entry.title === "string"
        ? normalizeConversationTitle(entry.title)
        : "";
      const icon = isConversationIcon(entry.icon) ? entry.icon : undefined;
      if (title || icon) customizations[id] = { title: title || undefined, icon };
    });
  }

  return {
    starts: normalizeStarts(candidate.starts),
    customizations,
  };
}

export function mergeConversationStarts(current: number[], discovered: number[]) {
  const merged = normalizeStarts([...current, ...discovered]);
  return merged.length === current.length
    && merged.every((value, index) => value === current[index])
    ? current
    : merged;
}

function readLocalState(): ConversationState {
  if (typeof localStorage === "undefined") return EMPTY_STATE;
  try {
    const current = localStorage.getItem(FALLBACK_KEY);
    if (current) return normalizeConversationState(JSON.parse(current));
    const legacyStarts = localStorage.getItem(LEGACY_STARTS_KEY);
    return normalizeConversationState({
      starts: legacyStarts ? JSON.parse(legacyStarts) : [],
      customizations: {},
    });
  } catch {
    return EMPTY_STATE;
  }
}

function writeLocalState(state: ConversationState) {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(FALLBACK_KEY, JSON.stringify(state));
}

export function conversationStateSnapshot() {
  return readLocalState();
}

export async function loadConversationState(): Promise<ConversationState> {
  const fallback = readLocalState();
  try {
    if (!isTauri()) return fallback;
    const { load } = await import("@tauri-apps/plugin-store");
    const store = await load(STORE_FILE, { autoSave: false });
    const stored = await store.get<unknown>(STORE_KEY);
    return stored ? normalizeConversationState(stored) : fallback;
  } catch {
    return fallback;
  }
}

export async function saveConversationState(state: ConversationState): Promise<void> {
  const normalized = normalizeConversationState(state);
  try {
    if (isTauri()) {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load(STORE_FILE, { autoSave: false });
      await store.set(STORE_KEY, normalized);
      await store.save();
      return;
    }
  } catch {
    // The browser-backed copy below keeps personalization available if the native store fails.
  }
  writeLocalState(normalized);
}
