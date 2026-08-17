
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

export interface CoreConversation {
  id: string;
  started_at: string;
  ended_at: string | null;
  automatic_title: string | null;
  custom_title: string | null;
  icon: string | null;
  subtitle_count: number;
  updated_at: string;
  active: boolean;
}

export interface ConversationCatalog {
  conversations: CoreConversation[];
}

export type ConversationSummary = {
  id: string;
  title: string;
  icon: ConversationIcon;
  customized: boolean;
  startedAt: string;
  endedAt: string | null;
  updatedAt: string;
  subtitleCount: number;
  active: boolean;
};


interface ConversationLabels {
  untitled: string;
  newConversation: string;
}

const DEFAULT_LABELS: ConversationLabels = {
  untitled: "未命名对话",
  newConversation: "新对话",
};

const VALID_ICONS = new Set<string>(CONVERSATION_ICON_KEYS);

export function isConversationIcon(value: unknown): value is ConversationIcon {
  return typeof value === "string" && VALID_ICONS.has(value);
}

function catalogTitle(
  conversation: CoreConversation,
  labels: ConversationLabels,
): string {
  const customTitle = conversation.custom_title?.trim();
  if (customTitle) return customTitle;
  const automaticTitle = conversation.automatic_title?.trim();
  if (automaticTitle) return automaticTitle;
  return conversation.active && conversation.subtitle_count === 0
    ? labels.newConversation
    : labels.untitled;
}

export function conversationsFromCatalog(
  catalog: ConversationCatalog | null,
  labels: ConversationLabels = DEFAULT_LABELS,
): ConversationSummary[] {
  if (catalog === null) return [];
  return catalog.conversations.map((conversation) => ({
    id: conversation.id,
    title: catalogTitle(conversation, labels),
    icon: isConversationIcon(conversation.icon) ? conversation.icon : "message",
    customized: Boolean(conversation.custom_title?.trim() || conversation.icon),
    startedAt: conversation.started_at,
    endedAt: conversation.ended_at,
    updatedAt: conversation.updated_at,
    subtitleCount: conversation.subtitle_count,
    active: conversation.active,
  }));
}
