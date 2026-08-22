import { request } from "../core-client/transport";
import type {
  Subtitle,
  SubtitleSearchPage,
  SubtitleTranslation,
} from "./types";

export const subtitlesApi = {
  subtitles: ({ limit = 100, beforeId }: { limit?: number; beforeId?: number } = {}) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (beforeId !== undefined) params.set("before_id", String(beforeId));
    return request<Subtitle[]>(`/api/subtitles?${params}`);
  },
  translateSubtitle: (subtitleId: number) => request<SubtitleTranslation>(
    `/api/subtitles/${subtitleId}/translation`,
    { method: "POST", body: JSON.stringify({}) },
  ),
  previewTranslation: (
    text: string,
    sourceLanguage?: string | null,
    targetLanguage?: string,
  ) => request<SubtitleTranslation>("/api/translations/preview", {
    method: "POST",
    body: JSON.stringify({
      text,
      source_language: sourceLanguage,
      target_language: targetLanguage,
    }),
  }),
  search: (
    query: string,
    { limit = 50, offset = 0, signal }: {
      limit?: number;
      offset?: number;
      signal?: AbortSignal;
    } = {},
  ) => {
    const params = new URLSearchParams({
      q: query,
      limit: String(limit),
      offset: String(offset),
    });
    return request<SubtitleSearchPage>(`/api/subtitles/search?${params}`, { signal });
  },
};
