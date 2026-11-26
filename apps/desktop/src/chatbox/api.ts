import { request } from "../core-client/transport";
import type { ChatboxPreferences } from "./chatbox";
import type { ChatboxComposeInput, ChatboxMessage, ChatboxPreview } from "./types";

export const chatboxApi = {
  previewChatbox: (input: ChatboxComposeInput) => request<ChatboxPreview>(
    "/api/chatbox/preview",
    { method: "POST", body: JSON.stringify(input) },
  ),
  sendChatbox: (input: ChatboxComposeInput) => request<ChatboxMessage>(
    "/api/chatbox/messages",
    { method: "POST", body: JSON.stringify(input) },
  ),
  syncPreferences: (preferences: ChatboxPreferences) => request<ChatboxPreferences>(
    "/api/chatbox/preferences",
    { method: "PUT", body: JSON.stringify(preferences) },
  ),
};
