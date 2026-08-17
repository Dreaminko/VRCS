import type { ConversationCatalog, CoreConversation } from "./conversations";

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
