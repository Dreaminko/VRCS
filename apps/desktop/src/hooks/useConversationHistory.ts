import { useCallback, useEffect, useRef, useState } from "react";

import { coreApi } from "../api";
import {
  conversationSubtitlePage,
  isAbortError,
  isConversationRequestCurrent,
  MAX_SUBTITLE_HISTORY_ITEMS,
  mergeSubtitleHistory,
  SUBTITLE_HISTORY_PAGE_SIZE,
  upsertSubtitleHistory,
  type ConversationRequestToken,
} from "../subtitle-stream";
import { clearTranslationPartial } from "../realtime-state";
import type { Subtitle, SubtitleTranslation } from "../types";

function updateSubtitle(
  subtitles: Subtitle[],
  subtitleId: number,
  update: (subtitle: Subtitle) => Subtitle,
): Subtitle[] {
  const index = subtitles.findIndex((subtitle) => subtitle.id === subtitleId);
  if (index < 0) return subtitles;
  const next = [...subtitles];
  next[index] = update(subtitles[index]);
  return next;
}

function withTranslation(
  subtitles: Subtitle[],
  subtitleId: number,
  translation: SubtitleTranslation,
): Subtitle[] {
  return updateSubtitle(subtitles, subtitleId, (subtitle) => ({
    ...subtitle,
    translations: [
      ...subtitle.translations.filter(
        (item) => item.target_language !== translation.target_language,
      ),
      translation,
    ],
    translation_partial: undefined,
  }));
}

function withoutTranslationPartial(
  subtitles: Subtitle[],
  subtitleId: number,
): Subtitle[] {
  return updateSubtitle(subtitles, subtitleId, (subtitle) => (
    subtitle.translation_partial
      ? { ...subtitle, translation_partial: undefined }
      : subtitle
  ));
}

export function useConversationHistory({
  coreConfigured,
  reportError,
  clearErrorFrom,
}: {
  coreConfigured: boolean;
  reportError: (reason: unknown, fallbackKey: string, source?: string) => void;
  clearErrorFrom: (source: string) => void;
}) {
  const [openedConversationId, setOpenedConversationId] = useState<string | null>(null);
  const [subtitles, setSubtitles] = useState<Subtitle[]>([]);
  const [hasOlderSubtitles, setHasOlderSubtitles] = useState(false);
  const [loadingConversationSubtitles, setLoadingConversationSubtitles] = useState(false);
  const [loadingOlderSubtitles, setLoadingOlderSubtitles] = useState(false);
  const [translatingSubtitleIds, setTranslatingSubtitleIds] = useState<number[]>([]);
  const currentConversationIdRef = useRef<string | null>(null);
  const nextBeforeIdRef = useRef<number | null>(null);
  const requestVersionRef = useRef(0);
  const paginationInitializedRef = useRef(false);
  const loadingOlderSubtitlesRef = useRef(false);
  const openRequestControllerRef = useRef<AbortController | null>(null);
  const olderRequestControllerRef = useRef<AbortController | null>(null);

  const requestIsCurrent = useCallback((request: ConversationRequestToken) => (
    isConversationRequestCurrent(
      request,
      currentConversationIdRef.current,
      requestVersionRef.current,
    )
  ), []);

  const abortRequests = useCallback(() => {
    openRequestControllerRef.current?.abort();
    olderRequestControllerRef.current?.abort();
    openRequestControllerRef.current = null;
    olderRequestControllerRef.current = null;
  }, []);

  const clearPool = useCallback((conversationId: string | null) => {
    abortRequests();
    currentConversationIdRef.current = conversationId;
    requestVersionRef.current += 1;
    nextBeforeIdRef.current = null;
    paginationInitializedRef.current = false;
    loadingOlderSubtitlesRef.current = false;
    setOpenedConversationId(conversationId);
    setSubtitles([]);
    setHasOlderSubtitles(false);
    setLoadingConversationSubtitles(false);
    setLoadingOlderSubtitles(false);
    setTranslatingSubtitleIds([]);
  }, [abortRequests]);

  const openConversation = useCallback(async (conversationId: string | null) => {
    clearPool(conversationId);
    if (!coreConfigured || conversationId === null) return;

    setLoadingConversationSubtitles(true);
    const request: ConversationRequestToken = {
      conversationId,
      version: requestVersionRef.current,
    };
    const controller = new AbortController();
    openRequestControllerRef.current = controller;
    try {
      const response = await coreApi.conversationSubtitles(conversationId, {
        limit: SUBTITLE_HISTORY_PAGE_SIZE,
        signal: controller.signal,
      });
      if (!requestIsCurrent(request)) return;
      const page = conversationSubtitlePage(response);
      setSubtitles((current) => mergeSubtitleHistory(current, page.items));
      setHasOlderSubtitles(page.hasOlder);
      nextBeforeIdRef.current = page.nextBeforeId;
      paginationInitializedRef.current = true;
      clearErrorFrom("subtitle-history");
    } catch (reason) {
      if (!isAbortError(reason) && requestIsCurrent(request)) {
        reportError(reason, "errors.core.connect", "subtitle-history");
      }
    } finally {
      if (openRequestControllerRef.current === controller) {
        openRequestControllerRef.current = null;
      }
      if (requestIsCurrent(request)) setLoadingConversationSubtitles(false);
    }
  }, [clearErrorFrom, clearPool, coreConfigured, reportError, requestIsCurrent]);

  const refreshOpenConversation = useCallback(async () => {
    const conversationId = currentConversationIdRef.current;
    if (
      !coreConfigured
      || conversationId === null
      || openRequestControllerRef.current !== null
    ) return;

    const request: ConversationRequestToken = {
      conversationId,
      version: requestVersionRef.current,
    };
    const controller = new AbortController();
    openRequestControllerRef.current = controller;
    try {
      const response = await coreApi.conversationSubtitles(conversationId, {
        limit: SUBTITLE_HISTORY_PAGE_SIZE,
        signal: controller.signal,
      });
      if (!requestIsCurrent(request)) return;
      const page = conversationSubtitlePage(response);
      setSubtitles((current) => mergeSubtitleHistory(current, page.items));
      if (!paginationInitializedRef.current) {
        setHasOlderSubtitles(page.hasOlder);
        nextBeforeIdRef.current = page.nextBeforeId;
        paginationInitializedRef.current = true;
      }
      clearErrorFrom("subtitle-history");
    } catch {
      // A reconnect refresh must not replace or clear the current pool.
    } finally {
      if (openRequestControllerRef.current === controller) {
        openRequestControllerRef.current = null;
      }
    }
  }, [clearErrorFrom, coreConfigured, requestIsCurrent]);

  const canLoadOlderSubtitles = hasOlderSubtitles
    && subtitles.length < MAX_SUBTITLE_HISTORY_ITEMS;

  const loadOlderSubtitles = useCallback(async () => {
    const conversationId = currentConversationIdRef.current;
    const beforeId = nextBeforeIdRef.current;
    if (
      conversationId === null
      || !canLoadOlderSubtitles
      || beforeId === null
      || loadingOlderSubtitlesRef.current
    ) return;

    const request: ConversationRequestToken = {
      conversationId,
      version: requestVersionRef.current,
    };
    const controller = new AbortController();
    olderRequestControllerRef.current = controller;
    loadingOlderSubtitlesRef.current = true;
    setLoadingOlderSubtitles(true);
    try {
      const response = await coreApi.conversationSubtitles(conversationId, {
        limit: SUBTITLE_HISTORY_PAGE_SIZE,
        beforeId,
        signal: controller.signal,
      });
      if (!requestIsCurrent(request)) return;
      const page = conversationSubtitlePage(response);
      setSubtitles((current) => mergeSubtitleHistory(current, page.items));
      setHasOlderSubtitles(page.hasOlder);
      nextBeforeIdRef.current = page.nextBeforeId;
      clearErrorFrom("subtitle-history");
    } catch (reason) {
      if (!isAbortError(reason) && requestIsCurrent(request)) {
        reportError(reason, "errors.core.connect", "subtitle-history");
      }
    } finally {
      if (olderRequestControllerRef.current === controller) {
        olderRequestControllerRef.current = null;
      }
      if (requestIsCurrent(request)) {
        loadingOlderSubtitlesRef.current = false;
        setLoadingOlderSubtitles(false);
      }
    }
  }, [canLoadOlderSubtitles, clearErrorFrom, reportError, requestIsCurrent]);

  const receiveSubtitle = useCallback((subtitle: Subtitle) => {
    if (subtitle.conversation_id !== currentConversationIdRef.current) return;
    setSubtitles((current) => upsertSubtitleHistory(current, subtitle));
  }, []);

  const translationStarted = useCallback((subtitleId: number) => {
    setSubtitles((current) => withoutTranslationPartial(current, subtitleId));
    setTranslatingSubtitleIds((current) => current.includes(subtitleId)
      ? current
      : [...current, subtitleId]);
  }, []);

  const translationCompleted = useCallback((
    subtitleId: number,
    translation: SubtitleTranslation,
  ) => {
    setSubtitles((current) => withTranslation(current, subtitleId, translation));
    setTranslatingSubtitleIds((current) => current.filter((id) => id !== subtitleId));
  }, []);

  const translationFailed = useCallback((subtitleId: number) => {
    setSubtitles((current) => withoutTranslationPartial(current, subtitleId));
    setTranslatingSubtitleIds((current) => current.filter((id) => id !== subtitleId));
  }, []);

  const translateSubtitle = useCallback(async (subtitleId: number) => {
    setTranslatingSubtitleIds((current) => current.includes(subtitleId)
      ? current
      : [...current, subtitleId]);
    try {
      const translation = await coreApi.translateSubtitle(subtitleId);
      clearTranslationPartial(subtitleId);
      setSubtitles((current) => withTranslation(current, subtitleId, translation));
      clearErrorFrom(`translation:${subtitleId}`);
    } catch (reason) {
      clearTranslationPartial(subtitleId);
      setSubtitles((current) => withoutTranslationPartial(current, subtitleId));
      reportError(
        reason,
        "errors.translation.failed",
        `translation:${subtitleId}`,
      );
    } finally {
      setTranslatingSubtitleIds((current) => current.filter((id) => id !== subtitleId));
    }
  }, [clearErrorFrom, reportError]);

  useEffect(() => {
    if (!coreConfigured) clearPool(null);
  }, [clearPool, coreConfigured]);

  useEffect(() => {
    if (!coreConfigured) return;
    const refreshHistory = () => {
      void openConversation(currentConversationIdRef.current);
    };
    window.addEventListener("vrcs:subtitle-history-refresh", refreshHistory);
    return () => window.removeEventListener("vrcs:subtitle-history-refresh", refreshHistory);
  }, [coreConfigured, openConversation]);

  useEffect(() => abortRequests, [abortRequests]);

  return {
    openedConversationId,
    subtitles,
    hasOlderSubtitles: canLoadOlderSubtitles,
    loadingConversationSubtitles,
    loadingOlderSubtitles,
    translatingSubtitleIds,
    openConversation,
    refreshOpenConversation,
    loadOlderSubtitles,
    receiveSubtitle,
    translationStarted,
    translationCompleted,
    translationFailed,
    translateSubtitle,
  };
}
