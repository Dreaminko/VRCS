import { request } from "../core-client/transport";
import type { DatabaseStorageStats } from "./types";

export const storageApi = {
  storageStats: () => request<DatabaseStorageStats>("/api/storage/stats"),
  clearSubtitleHistory: () => request<DatabaseStorageStats>("/api/subtitles", {
    method: "DELETE",
  }),
};
