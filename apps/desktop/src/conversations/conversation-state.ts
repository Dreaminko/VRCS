import type { Subtitle } from "../subtitles/types";
import type { ConversationCatalog, CoreConversation } from "./conversations";

export type ConversationSwitchDirection = "up" | "down" | "neutral";

export function conversationSwitchDirection(
  orderedIds: readonly string[],
  fromId: string | null | undefined,
  toId: string | null | undefined,
): ConversationSwitchDirection {
  if (!fromId || !toId || fromId === toId) return "neutral";
  const fromIndex = orderedIds.indexOf(fromId);
  const toIndex = orderedIds.indexOf(toId);
  if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return "neutral";
  return toIndex > fromIndex ? "down" : "up";
}

export interface CachedConversationPage {
  items: Subtitle[];
  hasOlder: boolean;
  nextBeforeId: number | null;
}

export const MAX_CACHED_CONVERSATIONS = 8;

export function conversationViewKey(
  conversationId: string | null,
  focusedSubtitleId: number | null = null,
): string | null {
  if (conversationId === null) return null;
  return focusedSubtitleId === null
    ? `${conversationId}:latest`
    : `${conversationId}:at:${focusedSubtitleId}`;
}

export function getCachedConversationPage(
  cache: Map<string, CachedConversationPage>,
  conversationId: string,
): CachedConversationPage | undefined {
  const entry = cache.get(conversationId);
  if (!entry) return undefined;
  cache.delete(conversationId);
  cache.set(conversationId, entry);
  return entry;
}

export function setCachedConversationPage(
  cache: Map<string, CachedConversationPage>,
  conversationId: string,
  page: CachedConversationPage,
): void {
  cache.delete(conversationId);
  cache.set(conversationId, page);
  while (cache.size > MAX_CACHED_CONVERSATIONS) {
    const oldestKey = cache.keys().next().value;
    if (oldestKey === undefined) break;
    cache.delete(oldestKey);
  }
}

export function updateCachedConversationPage(
  cache: Map<string, CachedConversationPage>,
  conversationId: string,
  updater: (items: Subtitle[]) => Subtitle[],
): void {
  const entry = cache.get(conversationId);
  if (!entry) return;
  const nextItems = updater(entry.items);
  if (nextItems !== entry.items) {
    cache.set(conversationId, { ...entry, items: nextItems });
  }
}

export function normalizeConversationTitle(value: string) {
  return Array.from(value.replace(/\s+/g, " ").trim()).slice(0, 40).join("");
}

export function activeConversationId(catalog: ConversationCatalog): string | null {
  return catalog.conversations.find((conversation) => conversation.active)?.id
    ?? catalog.conversations[0]?.id
    ?? null;
}

export function catalogConversation(
  catalog: ConversationCatalog | null,
  id: string,
): CoreConversation | undefined {
  return catalog?.conversations.find((conversation) => conversation.id === id);
}

export function selectedConversationIdForCatalog(
  catalog: ConversationCatalog,
  currentId: string | null,
): string | null {
  return currentId && catalogConversation(catalog, currentId)
    ? currentId
    : activeConversationId(catalog);
}

export function catalogAfterRequest(
  response: ConversationCatalog,
  requestStartSequence: number,
  latestEvent: { sequence: number; catalog: ConversationCatalog } | null,
): ConversationCatalog {
  return latestEvent && latestEvent.sequence > requestStartSequence
    ? latestEvent.catalog
    : response;
}
