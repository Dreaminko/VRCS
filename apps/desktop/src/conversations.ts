import type { Subtitle } from "./types";

const CONVERSATION_GAP_MS = 30 * 60 * 1000;

export const CONVERSATION_ICON_KEYS = [
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
] as const;

export type ConversationIcon = typeof CONVERSATION_ICON_KEYS[number];

export type ConversationCustomization = {
  title?: string;
  icon?: ConversationIcon;
};

export type SubtitleConversation = {
  id: string;
  title: string;
  icon: ConversationIcon;
  customized: boolean;
  startedAt: string;
  updatedAt: string;
  subtitles: Subtitle[];
};

export function conversationId(startedAt: number) {
  return `conversation-${startedAt}`;
}

interface ConversationLabels {
  untitled: string;
  newConversation: string;
}

const DEFAULT_LABELS: ConversationLabels = {
  untitled: "未命名对话",
  newConversation: "新对话",
};

function titleFrom(text: string, untitled: string) {
  return Array.from(text.replace(/\s+/g, " ").trim()).slice(0, 14).join("") || untitled;
}

export function groupConversations(
  subtitles: Subtitle[],
  manualStarts: number[],
  emptyStart: number,
  labels: ConversationLabels = DEFAULT_LABELS,
  customizations: Record<string, ConversationCustomization> = {},
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

  const grouped = starts.map(() => [] as Subtitle[]);
  let groupIndex = 0;
  ordered.forEach((subtitle) => {
    const createdAt = Date.parse(subtitle.created_at);
    while (
      groupIndex + 1 < starts.length
      && createdAt >= starts[groupIndex + 1]
    ) {
      groupIndex += 1;
    }
    if (createdAt >= starts[groupIndex]) grouped[groupIndex].push(subtitle);
  });

  return starts
    .map((startedAt, index) => {
      const items = grouped[index];
      const first = items[0];
      const last = items[items.length - 1];
      const id = conversationId(startedAt);
      const customization = customizations[id];
      return {
        id,
        title: customization?.title
          ?? (first ? titleFrom(first.text, labels.untitled) : labels.newConversation),
        icon: customization?.icon ?? "message",
        customized: Boolean(customization?.title || customization?.icon),
        startedAt: new Date(startedAt).toISOString(),
        updatedAt: last?.created_at ?? new Date(startedAt).toISOString(),
        subtitles: [...items].reverse(),
      };
    })
    .filter((conversation, index, conversations) => conversation.subtitles.length || index === conversations.length - 1)
    .reverse();
}
