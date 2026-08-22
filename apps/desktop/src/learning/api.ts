import { request } from "../core-client/transport";
import type {
  LearningAnalysisInput,
  LearningDraftInput,
  LearningItem,
  LearningItemCreateInput,
  LearningItemPatchInput,
  LearningItemStatus,
  SelectionQueryInput,
  SelectionQueryResponse,
} from "./types";

const AI_REQUEST_TIMEOUT_MS = 125_000;

export const learningApi = {
  learningItems: ({
    status,
    limit = 50,
    beforeId,
  }: {
    status?: LearningItemStatus;
    limit?: number;
    beforeId?: number;
  } = {}) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (status) params.set("status", status);
    if (beforeId !== undefined) params.set("before_id", String(beforeId));
    return request<LearningItem[]>(`/api/learning/items?${params}`);
  },
  learningCaptureKeys: () => request<{ keys: string[] }>("/api/learning/capture-keys"),
  createLearningItem: (input: LearningItemCreateInput) => request<LearningItem>(
    "/api/learning/items",
    { method: "POST", body: JSON.stringify(input) },
  ),
  updateLearningItem: (itemId: number, input: LearningItemPatchInput) => request<LearningItem>(
    `/api/learning/items/${itemId}`,
    { method: "PATCH", body: JSON.stringify(input) },
  ),
  archiveLearningItem: (itemId: number) => request<LearningItem>(
    `/api/learning/items/${itemId}/archive`,
    { method: "POST" },
  ),
  restoreLearningItem: (itemId: number) => request<LearningItem>(
    `/api/learning/items/${itemId}/restore`,
    { method: "POST" },
  ),
  deleteLearningItem: (itemId: number) => request<{ deleted: boolean }>(
    `/api/learning/items/${itemId}`,
    { method: "DELETE" },
  ),
  analyzeLearningItem: (itemId: number, input: LearningAnalysisInput) => request<LearningItem>(
    `/api/learning/items/${itemId}/analysis`,
    { method: "POST", body: JSON.stringify(input) },
  ),
  querySelection: (input: SelectionQueryInput, signal?: AbortSignal) => request<SelectionQueryResponse>(
    "/api/learning/selection-query",
    {
      method: "POST",
      body: JSON.stringify(input),
      signal,
      timeoutMs: AI_REQUEST_TIMEOUT_MS,
    },
  ),
  generateLearningDraft: (itemId: number, input: LearningDraftInput) => request<LearningItem>(
    `/api/learning/items/${itemId}/draft`,
    { method: "POST", body: JSON.stringify(input) },
  ),
  exportLearningItem: (itemId: number) => request<LearningItem>(
    `/api/learning/items/${itemId}/export`,
    { method: "POST" },
  ),
};
