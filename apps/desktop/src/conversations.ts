import type { Subtitle } from "./types";

export const CONVERSATION_GAP_MS = 30 * 60 * 1000;

export type SubtitleConversation = {
  id: string;
  title: string;
  startedAt: string;
  updatedAt: string;
  subtitles: Subtitle[];
};

export function conversationId(startedAt: number) {
  return `conversation-${startedAt}`;
}

function titleFrom(text: string) {
  return Array.from(text.replace(/\s+/g, " ").trim()).slice(0, 14).join("") || "未命名对话";
}

export function groupConversations(
  subtitles: Subtitle[],
  manualStarts: number[],
  emptyStart: number,
): SubtitleConversation[] {
  const ordered = subtitles
    .filter((subtitle) => Number.isFinite(Date.parse(subtitle.created_at)))
    .sort((left, right) => Date.parse(left.created_at) - Date.parse(right.created_at));
  const boundaries = [...new Set(manualStarts.filter(Number.isFinite))].sort((left, right) => left - right);
  const naturalStarts: number[] = [];

  ordered.forEach((subtitle, index) => {
    const createdAt = Date.parse(subtitle.created_at);
    const previousAt = index ? Date.parse(ordered[index - 1].created_at) : Number.NEGATIVE_INFINITY;
    const hasManualBoundary = boundaries.some((boundary) => boundary > previousAt && boundary <= createdAt);
    if ((!index || createdAt - previousAt > CONVERSATION_GAP_MS) && !hasManualBoundary) {
      naturalStarts.push(createdAt);
    }
  });

  const starts = [...new Set([...boundaries, ...naturalStarts])].sort((left, right) => left - right);
  if (!starts.length) starts.push(emptyStart);

  return starts
    .map((startedAt, index) => {
      const nextStart = starts[index + 1] ?? Number.POSITIVE_INFINITY;
      const items = ordered.filter((subtitle) => {
        const createdAt = Date.parse(subtitle.created_at);
        return createdAt >= startedAt && createdAt < nextStart;
      });
      const first = items[0];
      const last = items[items.length - 1];
      return {
        id: conversationId(startedAt),
        title: first ? titleFrom(first.text) : "新对话",
        startedAt: new Date(startedAt).toISOString(),
        updatedAt: last?.created_at ?? new Date(startedAt).toISOString(),
        subtitles: [...items].reverse(),
      };
    })
    .filter((conversation, index, conversations) => conversation.subtitles.length || index === conversations.length - 1)
    .reverse();
}
