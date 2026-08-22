import { request } from "../core-client/transport";
import type { DatabaseStorageStats } from "./types";

export const storageApi = {
  storageStats: () => request<DatabaseStorageStats>("/api/storage/stats"),
  clearSubtitleHistory: () => request<DatabaseStorageStats>("/api/subtitles", {
    method: "DELETE",
  }),
  deleteSubtitleRange: (startedAt: string, endedAt?: string) => request<{ deleted: number }>(
    "/api/subtitles/range",
    {
      method: "DELETE",
      body: JSON.stringify({ started_at: startedAt, ended_at: endedAt }),
    },
  ),
};
