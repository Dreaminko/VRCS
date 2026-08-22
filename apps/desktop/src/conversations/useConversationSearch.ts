import { useCallback, useEffect, useRef, useState } from "react";

import { subtitlesApi } from "../subtitles/api";
import type { SubtitleSearchHit } from "../subtitles/types";
import {
  isSubtitleSearchable,
  mergeSubtitleSearchHits,
} from "../subtitle-search";

const SEARCH_DELAY_MS = 250;
const SEARCH_PAGE_SIZE = 50;

export function useConversationSearch(enabled: boolean) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<SubtitleSearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [failed, setFailed] = useState(false);
  const requestVersionRef = useRef(0);
  const requestControllerRef = useRef<AbortController | null>(null);
  const normalizedQuery = query.trim();
  const searchable = isSubtitleSearchable(normalizedQuery);

  const cancelRequest = useCallback(() => {
    requestControllerRef.current?.abort();
    requestControllerRef.current = null;
  }, []);

  useEffect(() => {
    cancelRequest();
    requestVersionRef.current += 1;
    const version = requestVersionRef.current;
    setItems([]);
    setHasMore(false);
    setFailed(false);
    setLoading(false);
    setLoadingMore(false);
    if (!enabled || !searchable) return;

    const timer = window.setTimeout(async () => {
      const controller = new AbortController();
      requestControllerRef.current = controller;
      setLoading(true);
      try {
        const page = await subtitlesApi.search(normalizedQuery, {
          limit: SEARCH_PAGE_SIZE,
          signal: controller.signal,
        });
        if (version !== requestVersionRef.current) return;
        setItems(page.items);
        setHasMore(page.has_more);
      } catch (reason) {
        if (controller.signal.aborted || version !== requestVersionRef.current) return;
        setFailed(true);
      } finally {
        if (requestControllerRef.current === controller) {
          requestControllerRef.current = null;
        }
        if (version === requestVersionRef.current) setLoading(false);
      }
    }, SEARCH_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [cancelRequest, enabled, normalizedQuery, searchable]);

  const loadMore = useCallback(async () => {
    if (!enabled || !searchable || loading || loadingMore || !hasMore) return;
    const version = requestVersionRef.current;
    const controller = new AbortController();
    requestControllerRef.current = controller;
    setLoadingMore(true);
    try {
      const page = await subtitlesApi.search(normalizedQuery, {
        limit: SEARCH_PAGE_SIZE,
        offset: items.length,
        signal: controller.signal,
      });
      if (version !== requestVersionRef.current) return;
      setItems((current) => mergeSubtitleSearchHits(current, page.items));
      setHasMore(page.has_more);
      setFailed(false);
    } catch {
      if (!controller.signal.aborted && version === requestVersionRef.current) {
        setFailed(true);
      }
    } finally {
      if (requestControllerRef.current === controller) requestControllerRef.current = null;
      if (version === requestVersionRef.current) setLoadingMore(false);
    }
  }, [enabled, hasMore, items.length, loading, loadingMore, normalizedQuery, searchable]);

  useEffect(() => cancelRequest, [cancelRequest]);

  return {
    query,
    setQuery,
    items,
    loading,
    loadingMore,
    hasMore,
    failed,
    searchable,
    active: query.length > 0,
    clear: () => setQuery(""),
    loadMore,
  };
}
