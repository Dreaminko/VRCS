import { request } from "../core-client/transport";
import type { ConversationSubtitlePage } from "../subtitle-stream";
import type { ConversationSubtitleContext } from "../subtitles/types";
import type {
  ConversationCatalog,
  ConversationIcon,
} from "./conversations";

export const conversationsApi = {
  conversations: () => request<ConversationCatalog>("/api/conversations"),
  createConversation: () => request<ConversationCatalog>("/api/conversations", {
    method: "POST",
    body: JSON.stringify({}),
  }),
  updateConversation: (
    conversationId: string,
    input: { custom_title?: string | null; icon?: ConversationIcon | null },
  ) => request<ConversationCatalog>(
    `/api/conversations/${encodeURIComponent(conversationId)}`,
    { method: "PATCH", body: JSON.stringify(input) },
  ),
  deleteConversation: (conversationId: string) => request<ConversationCatalog>(
    `/api/conversations/${encodeURIComponent(conversationId)}`,
    { method: "DELETE" },
  ),
  conversationSubtitles: (
    conversationId: string,
    {
      limit = 100,
      beforeId,
      signal,
    }: { limit?: number; beforeId?: number; signal?: AbortSignal } = {},
  ) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (beforeId !== undefined) params.set("before_id", String(beforeId));
    return request<ConversationSubtitlePage>(
      `/api/conversations/${encodeURIComponent(conversationId)}/subtitles?${params}`,
      { signal },
    );
  },
  conversationSubtitleContext: (
    conversationId: string,
    subtitleId: number,
    { radius = 50, signal }: { radius?: number; signal?: AbortSignal } = {},
  ) => request<ConversationSubtitleContext>(
    `/api/conversations/${encodeURIComponent(conversationId)}/subtitles/${subtitleId}/context?radius=${radius}`,
    { signal },
  ),
};
