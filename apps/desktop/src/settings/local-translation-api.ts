import { request } from "../core-client/transport";
import type { LocalTranslationStatus } from "./types";

export const localTranslationApi = {
  status: () => request<LocalTranslationStatus>("/api/local-translation"),
  download: (id: string, revision: string) => request<LocalTranslationStatus>(
    "/api/local-translation/download",
    { method: "POST", body: JSON.stringify({ id, revision }) },
  ),
  cancel: () => request<LocalTranslationStatus>(
    "/api/local-translation/cancel",
    { method: "POST" },
  ),
  delete: (id: string) => request<{ deleted: boolean }>(
    `/api/local-translation/models/${encodeURIComponent(id)}`,
    { method: "DELETE" },
  ),
};
