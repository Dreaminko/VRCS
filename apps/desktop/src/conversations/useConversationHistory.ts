import { useCallback, useEffect, useRef, useState } from "react";

import { subtitlesApi } from "../subtitles/api";
import { conversationsApi } from "./api";
import {
  conversationViewKey,
  getCachedConversationPage,
  setCachedConversationPage,
  updateCachedConversationPage,
  type CachedConversationPage,
} from "./conversation-state";
import {
  conversationSubtitlePage,
  isAbortError,
  isConversationRequestCurrent,
  MAX_SUBTITLE_HISTORY_ITEMS,
  mergeSubtitleHistory,
  SUBTITLE_HISTORY_PAGE_SIZE,
  upsertSubtitleHistory,
  type ConversationRequestToken,
} from "../subtitles/subtitle-stream";
import { clearTranslationPartial } from "../core-client/realtime-state";
import type {
  Subtitle,
  SubtitleTranslation,
} from "../subtitles/types";

function updateSubtitle(
  subtitles: Subtitle[],
  subtitleId: number,
  update: (subtitle: Subtitle) => Subtitle,
): Subtitle[] {
  const index = subtitles.findIndex((subtitle) => subtitle.id === subtitleId);
  if (index < 0) return subtitles;
  const updated = update(subtitles[index]);
  if (updated === subtitles[index]) return subtitles;
  const next = [...subtitles];
  next[index] = updated;
  return next;
}

function withTranslation(
  subtitles: Subtitle[],
  subtitleId: number,
  translation: SubtitleTranslation,
  preferred = false,
): Subtitle[] {
  return updateSubtitle(subtitles, subtitleId, (subtitle) => ({
    ...subtitle,
    translations: preferred
      ? [
        translation,
        ...subtitle.translations.filter(
          (item) => item.target_language !== translation.target_language,
        ),
      ]
      : [
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
  const [targetViewKey, setTargetViewKey] = useState<string | null>(null);
  const [committedViewKey, setCommittedViewKey] = useState<string | null>(null);
  const [subtitles, setSubtitles] = useState<Subtitle[]>([]);
  const [hasOlderSubtitles, setHasOlderSubtitles] = useState(false);
  const [loadingConversationSubtitles, setLoadingConversationSubtitles] = useState(false);
  const [loadingOlderSubtitles, setLoadingOlderSubtitles] = useState(false);
  const [focusedSubtitleId, setFocusedSubtitleId] = useState<number | null>(null);
  const [translatingSubtitleIds, setTranslatingSubtitleIds] = useState<number[]>([]);
  const currentConversationIdRef = useRef<string | null>(null);
  const openedConversationIdRef = useRef<string | null>(null);
  const focusedSubtitleIdRef = useRef<number | null>(null);
  const nextBeforeIdRef = useRef<number | null>(null);
  const requestVersionRef = useRef(0);
  const paginationInitializedRef = useRef(false);
  const loadingOlderSubtitlesRef = useRef(false);
  const openRequestControllerRef = useRef<AbortController | null>(null);
  const olderRequestControllerRef = useRef<AbortController | null>(null);
  const cacheRef = useRef<Map<string, CachedConversationPage>>(new Map());
  const pendingLiveSubtitlesRef = useRef<Subtitle[]>([]);

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

  const commitConversation = useCallback(({
    conversationId,
    items,
    hasOlder,
    nextBeforeId,
    focusedSubtitleId: nextFocusedId,
    viewKey,
  }: {
    conversationId: string | null;
    items: Subtitle[];
    hasOlder: boolean;
    nextBeforeId: number | null;
    focusedSubtitleId: number | null;
    viewKey: string | null;
  }) => {
    openedConversationIdRef.current = conversationId;
    nextBeforeIdRef.current = nextBeforeId;
    paginationInitializedRef.current = conversationId !== null;
    focusedSubtitleIdRef.current = nextFocusedId;
    loadingOlderSubtitlesRef.current = false;
    setOpenedConversationId(conversationId);
    setSubtitles(items);
    setHasOlderSubtitles(hasOlder);
    setLoadingOlderSubtitles(false);
    setFocusedSubtitleId(nextFocusedId);
    setTranslatingSubtitleIds([]);
    setCommittedViewKey(viewKey);
  }, []);

  const clearPool = useCallback((conversationId: string | null) => {
    abortRequests();
    currentConversationIdRef.current = conversationId;
    requestVersionRef.current += 1;
    cacheRef.current.clear();
    pendingLiveSubtitlesRef.current = [];
    setTargetViewKey(conversationViewKey(conversationId, null));
    setLoadingConversationSubtitles(false);
    commitConversation({
      conversationId,
      items: [],
      hasOlder: false,
      nextBeforeId: null,
      focusedSubtitleId: null,
      viewKey: conversationViewKey(conversationId, null),
    });
  }, [abortRequests, commitConversation]);

  const openConversation = useCallback(async (conversationId: string | null) => {
    if (!coreConfigured || conversationId === null) {
      clearPool(null);
      return;
    }

    abortRequests();
    currentConversationIdRef.current = conversationId;
    requestVersionRef.current += 1;
    pendingLiveSubtitlesRef.current = [];
    const viewKey = conversationViewKey(conversationId, null);
    setTargetViewKey(viewKey);

    const request: ConversationRequestToken = {
      conversationId,
      version: requestVersionRef.current,
    };
    const cached = getCachedConversationPage(cacheRef.current, conversationId);
    if (cached) {
      commitConversation({
        conversationId,
        items: cached.items,
        hasOlder: cached.hasOlder,
        nextBeforeId: cached.nextBeforeId,
        focusedSubtitleId: null,
        viewKey,
      });
      setLoadingConversationSubtitles(false);

      const controller = new AbortController();
      openRequestControllerRef.current = controller;
      try {
        const response = await conversationsApi.conversationSubtitles(conversationId, {
          limit: SUBTITLE_HISTORY_PAGE_SIZE,
          signal: controller.signal,
        });
        if (!requestIsCurrent(request)) return;
        const page = conversationSubtitlePage(response);
        setSubtitles((current) => {
          const merged = mergeSubtitleHistory(current, page.items);
          const hasOlder = page.hasOlder || nextBeforeIdRef.current !== null;
          const nextBeforeId = nextBeforeIdRef.current ?? page.nextBeforeId;
          setHasOlderSubtitles(hasOlder);
          nextBeforeIdRef.current = nextBeforeId;
          setCachedConversationPage(cacheRef.current, conversationId, {
            items: merged,
            hasOlder,
            nextBeforeId,
          });
          return merged;
        });
        clearErrorFrom("subtitle-history");
      } catch (reason) {
        if (!isAbortError(reason)) {
          console.warn("Failed to revalidate cached conversation history", reason);
        }
      } finally {
        if (openRequestControllerRef.current === controller) {
          openRequestControllerRef.current = null;
        }
      }
      return;
    }

    setLoadingConversationSubtitles(true);
    const controller = new AbortController();
    openRequestControllerRef.current = controller;
    try {
      const response = await conversationsApi.conversationSubtitles(conversationId, {
        limit: SUBTITLE_HISTORY_PAGE_SIZE,
        signal: controller.signal,
      });
      if (!requestIsCurrent(request)) return;
      const page = conversationSubtitlePage(response);
      const merged = mergeSubtitleHistory(pendingLiveSubtitlesRef.current, page.items);
      pendingLiveSubtitlesRef.current = [];
      setCachedConversationPage(cacheRef.current, conversationId, {
        items: merged,
        hasOlder: page.hasOlder,
        nextBeforeId: page.nextBeforeId,
      });
      commitConversation({
        conversationId,
        items: merged,
        hasOlder: page.hasOlder,
        nextBeforeId: page.nextBeforeId,
        focusedSubtitleId: null,
        viewKey,
      });
      clearErrorFrom("subtitle-history");
    } catch (reason) {
      if (!isAbortError(reason) && requestIsCurrent(request)) {
        reportError(reason, "errors.core.connect", "subtitle-history");
        commitConversation({
          conversationId,
          items: [],
          hasOlder: false,
          nextBeforeId: null,
          focusedSubtitleId: null,
          viewKey,
        });
      }
    } finally {
      if (openRequestControllerRef.current === controller) {
        openRequestControllerRef.current = null;
      }
      if (requestIsCurrent(request)) setLoadingConversationSubtitles(false);
    }
  }, [abortRequests, clearErrorFrom, clearPool, commitConversation, coreConfigured, reportError, requestIsCurrent]);

  const openConversationAt = useCallback(async (
    conversationId: string,
    subtitleId: number,
  ) => {
    if (!coreConfigured) {
      clearPool(null);
      return;
    }

    abortRequests();
    currentConversationIdRef.current = conversationId;
    requestVersionRef.current += 1;
    pendingLiveSubtitlesRef.current = [];
    const viewKey = conversationViewKey(conversationId, subtitleId);
    setTargetViewKey(viewKey);
    setLoadingConversationSubtitles(true);

    const request: ConversationRequestToken = {
      conversationId,
      version: requestVersionRef.current,
    };
    const controller = new AbortController();
    openRequestControllerRef.current = controller;
    try {
      const context = await conversationsApi.conversationSubtitleContext(
        conversationId,
        subtitleId,
        { signal: controller.signal },
      );
      if (!requestIsCurrent(request)) return;
      commitConversation({
        conversationId,
        items: context.items,
        hasOlder: context.has_older,
        nextBeforeId: context.has_older ? context.items.at(-1)?.id ?? null : null,
        focusedSubtitleId: subtitleId,
        viewKey,
      });
      clearErrorFrom("subtitle-history");
    } catch (reason) {
      if (!isAbortError(reason) && requestIsCurrent(request)) {
        reportError(reason, "errors.core.connect", "subtitle-history");
        commitConversation({
          conversationId,
          items: [],
          hasOlder: false,
          nextBeforeId: null,
          focusedSubtitleId: subtitleId,
          viewKey,
        });
      }
    } finally {
      if (openRequestControllerRef.current === controller) {
        openRequestControllerRef.current = null;
      }
      if (requestIsCurrent(request)) setLoadingConversationSubtitles(false);
    }
  }, [abortRequests, clearErrorFrom, clearPool, commitConversation, coreConfigured, reportError, requestIsCurrent]);

  const refreshOpenConversation = useCallback(async () => {
    const conversationId = currentConversationIdRef.current;
    if (
      !coreConfigured
      || conversationId === null
      || focusedSubtitleIdRef.current !== null
      || openRequestControllerRef.current !== null
    ) return;

    const request: ConversationRequestToken = {
      conversationId,
      version: requestVersionRef.current,
    };
    const controller = new AbortController();
    openRequestControllerRef.current = controller;
    try {
      const response = await conversationsApi.conversationSubtitles(conversationId, {
        limit: SUBTITLE_HISTORY_PAGE_SIZE,
        signal: controller.signal,
      });
      if (!requestIsCurrent(request)) return;
      const page = conversationSubtitlePage(response);
      const nextHasOlder = !paginationInitializedRef.current
        ? page.hasOlder
        : page.hasOlder || nextBeforeIdRef.current !== null;
      const nextBeforeId = !paginationInitializedRef.current
        ? page.nextBeforeId
        : nextBeforeIdRef.current ?? page.nextBeforeId;
      if (!paginationInitializedRef.current) {
        setHasOlderSubtitles(nextHasOlder);
        nextBeforeIdRef.current = nextBeforeId;
        paginationInitializedRef.current = true;
      }
      setSubtitles((current) => {
        const merged = mergeSubtitleHistory(current, page.items);
        setCachedConversationPage(cacheRef.current, conversationId, {
          items: merged,
          hasOlder: nextHasOlder,
          nextBeforeId,
        });
        return merged;
      });
      clearErrorFrom("subtitle-history");
    } catch (reason) {
      // A reconnect refresh must not replace or clear the current pool.
      if (!isAbortError(reason)) {
        console.warn("Failed to refresh open conversation history", reason);
      }
    } finally {
      if (openRequestControllerRef.current === controller) {
        openRequestControllerRef.current = null;
      }
    }
  }, [clearErrorFrom, coreConfigured, requestIsCurrent]);

  const canLoadOlderSubtitles = hasOlderSubtitles
    && subtitles.length < MAX_SUBTITLE_HISTORY_ITEMS;

  const loadOlderSubtitles = useCallback(async () => {
    const conversationId = openedConversationIdRef.current;
    const beforeId = nextBeforeIdRef.current;
    if (
      conversationId === null
      || conversationId !== currentConversationIdRef.current
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
      const response = await conversationsApi.conversationSubtitles(conversationId, {
        limit: SUBTITLE_HISTORY_PAGE_SIZE,
        beforeId,
        signal: controller.signal,
      });
      if (!requestIsCurrent(request)) return;
      const page = conversationSubtitlePage(response);
      setHasOlderSubtitles(page.hasOlder);
      nextBeforeIdRef.current = page.nextBeforeId;
      setSubtitles((current) => {
        const merged = mergeSubtitleHistory(current, page.items);
        if (focusedSubtitleIdRef.current === null) {
          setCachedConversationPage(cacheRef.current, conversationId, {
            items: merged,
            hasOlder: page.hasOlder,
            nextBeforeId: page.nextBeforeId,
          });
        }
        return merged;
      });
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
    const subtitleConversationId = subtitle.conversation_id;
    if (!subtitleConversationId) return;

    if (
      focusedSubtitleIdRef.current === null
      && subtitleConversationId === openedConversationIdRef.current
    ) {
      setSubtitles((current) => upsertSubtitleHistory(current, subtitle));
      updateCachedConversationPage(
        cacheRef.current,
        subtitleConversationId,
        (items) => upsertSubtitleHistory(items, subtitle),
      );
      return;
    }

    if (
      subtitleConversationId === currentConversationIdRef.current
      && currentConversationIdRef.current !== openedConversationIdRef.current
    ) {
      pendingLiveSubtitlesRef.current = upsertSubtitleHistory(
        pendingLiveSubtitlesRef.current,
        subtitle,
      );
      return;
    }

    updateCachedConversationPage(
      cacheRef.current,
      subtitleConversationId,
      (items) => upsertSubtitleHistory(items, subtitle),
    );
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
    preferred: boolean,
  ) => {
    setSubtitles((current) => withTranslation(current, subtitleId, translation, preferred));
    const conversationId = openedConversationIdRef.current;
    if (conversationId !== null && focusedSubtitleIdRef.current === null) {
      updateCachedConversationPage(
        cacheRef.current,
        conversationId,
        (items) => withTranslation(items, subtitleId, translation, preferred),
      );
    }
    setTranslatingSubtitleIds((current) => current.filter((id) => id !== subtitleId));
  }, []);

  const translationFailed = useCallback((subtitleId: number) => {
    setSubtitles((current) => withoutTranslationPartial(current, subtitleId));
    setTranslatingSubtitleIds((current) => current.filter((id) => id !== subtitleId));
  }, []);

  const translateSubtitle = useCallback(async (subtitleId: number) => {
    clearErrorFrom(`translation:${subtitleId}`);
    setTranslatingSubtitleIds((current) => current.includes(subtitleId)
      ? current
      : [...current, subtitleId]);
    try {
      const translation = await subtitlesApi.translateSubtitle(subtitleId);
      clearTranslationPartial(subtitleId);
      setSubtitles((current) => withTranslation(current, subtitleId, translation, true));
      const conversationId = openedConversationIdRef.current;
      if (conversationId !== null && focusedSubtitleIdRef.current === null) {
        updateCachedConversationPage(
          cacheRef.current,
          conversationId,
          (items) => withTranslation(items, subtitleId, translation, true),
        );
      }
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
      cacheRef.current.clear();
      void openConversation(currentConversationIdRef.current);
    };
    window.addEventListener("vrcs:subtitle-history-refresh", refreshHistory);
    return () => window.removeEventListener("vrcs:subtitle-history-refresh", refreshHistory);
  }, [coreConfigured, openConversation]);

  useEffect(() => abortRequests, [abortRequests]);

  return {
    openedConversationId,
    targetViewKey,
    committedViewKey,
    subtitles,
    hasOlderSubtitles: canLoadOlderSubtitles,
    loadingConversationSubtitles,
    loadingOlderSubtitles,
    focusedSubtitleId,
    translatingSubtitleIds,
    openConversation,
    openConversationAt,
    refreshOpenConversation,
    loadOlderSubtitles,
    receiveSubtitle,
    translationStarted,
    translationCompleted,
    translationFailed,
    translateSubtitle,
  };
}
