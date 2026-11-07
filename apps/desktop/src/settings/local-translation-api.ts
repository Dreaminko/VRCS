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
  verify: () => request<LocalTranslationStatus>(
    "/api/local-translation/verify",
    { method: "POST" },
  ),
  unload: () => request<LocalTranslationStatus>(
    "/api/local-translation/unload",
    { method: "POST" },
  ),
  delete: (id: string) => request<{ deleted: boolean }>(
    `/api/local-translation/models/${encodeURIComponent(id)}`,
    { method: "DELETE" },
  ),
};
