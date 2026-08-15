export const DEFAULT_CONVERSATION_SIDEBAR_WIDTH = 268;
export const MIN_CONVERSATION_SIDEBAR_WIDTH = 160;
export const MAX_CONVERSATION_SIDEBAR_WIDTH = 480;

export function normalizeConversationSidebarWidth(value: unknown): number {
  const parsed = typeof value === "number" ? value : Number(value);
  const width = Number.isFinite(parsed)
    ? parsed
    : DEFAULT_CONVERSATION_SIDEBAR_WIDTH;
  return Math.round(Math.min(
    MAX_CONVERSATION_SIDEBAR_WIDTH,
    Math.max(MIN_CONVERSATION_SIDEBAR_WIDTH, width),
  ));
}
