import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { UIEvent } from "react";
import { useTranslation } from "react-i18next";

import type { Page } from "../app-types";
import {
  conversationId,
  groupConversations,
  type ConversationCustomization,
  type ConversationIcon,
} from "../conversations";
import {
  conversationStateSnapshot,
  loadConversationState,
  mergeConversationStarts,
  normalizeConversationTitle,
  saveConversationState,
} from "../conversation-state";
import { shouldFollowLiveScroll } from "../live-scroll";
import type { Subtitle } from "../types";

const SIDEBAR_OPEN_KEY = "vrcs.conversation-sidebar-open";

export function useConversationWorkspace({
  subtitles,
  page,
  running,
}: {
  subtitles: Subtitle[];
  page: Page;
  running: boolean;
}) {
  const { t, i18n } = useTranslation();
  const openedAt = useRef(Date.now()).current;
  const [sidebarOpen, setSidebarOpen] = useState(
    () => localStorage.getItem(SIDEBAR_OPEN_KEY) !== "false",
  );
  const initialConversationState = useRef(conversationStateSnapshot()).current;
  const [conversationStarts, setConversationStarts] = useState(
    initialConversationState.starts,
  );
  const [customizations, setCustomizations] = useState<
    Record<string, ConversationCustomization>
  >(initialConversationState.customizations);
  const [conversationStateReady, setConversationStateReady] = useState(false);
  const [selectedConversationId, setSelectedConversationId] = useState<
    string | null
  >(null);
  const conversations = useMemo(
    () => groupConversations(subtitles, conversationStarts, openedAt, {
      untitled: t("conversations.untitled"),
      newConversation: t("conversations.new"),
    }, customizations),
    [conversationStarts, customizations, i18n.resolvedLanguage, openedAt, subtitles, t],
  );
  const activeConversation = conversations[0];
  const selectedConversation = conversations.find(
    (conversation) => conversation.id === selectedConversationId,
  ) ?? activeConversation;
  const liveScrollRef = useRef<HTMLDivElement>(null);
  const previousLiveScrollTopRef = useRef(0);
  const [followingLiveSubtitles, setFollowingLiveSubtitles] = useState(true);
  const showingActiveConversation = (
    selectedConversation?.id === activeConversation?.id
  );
  const liveAutoScrollActive = (
    page === "live" && running && showingActiveConversation
  );

  const scrollLiveViewToBottom = useCallback(
    (behavior: ScrollBehavior = "smooth") => {
      const scrollRegion = liveScrollRef.current;
      if (!scrollRegion) return;
      setFollowingLiveSubtitles(true);
      previousLiveScrollTopRef.current = scrollRegion.scrollTop;
      scrollRegion.scrollTo({ top: scrollRegion.scrollHeight, behavior });
    },
    [],
  );

  useLayoutEffect(() => {
    if (page === "live") return;
    const scrollRegion = liveScrollRef.current;
    if (!scrollRegion) return;
    scrollRegion.scrollTop = 0;
    previousLiveScrollTopRef.current = 0;
  }, [page]);

  useEffect(() => {
    if (page !== "live") return;
    setFollowingLiveSubtitles(true);
    const frame = window.requestAnimationFrame(
      () => scrollLiveViewToBottom("auto"),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [page, scrollLiveViewToBottom, selectedConversation?.id]);

  useEffect(() => {
    if (!liveAutoScrollActive || !followingLiveSubtitles) return;
    const frame = window.requestAnimationFrame(
      () => scrollLiveViewToBottom(),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [
    followingLiveSubtitles,
    liveAutoScrollActive,
    scrollLiveViewToBottom,
    selectedConversation?.updatedAt,
  ]);

  useEffect(() => {
    localStorage.setItem(SIDEBAR_OPEN_KEY, String(sidebarOpen));
  }, [sidebarOpen]);

  useEffect(() => {
    let active = true;
    void loadConversationState().then((state) => {
      if (!active) return;
      setConversationStarts(state.starts);
      setCustomizations(state.customizations);
      setConversationStateReady(true);
    });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!conversationStateReady) return;
    const timer = window.setTimeout(() => {
      void saveConversationState({
        starts: conversationStarts,
        customizations,
      });
    }, 120);
    return () => window.clearTimeout(timer);
  }, [conversationStarts, conversationStateReady, customizations]);

  useEffect(() => {
    const discovered = conversations
      .map((conversation) => Date.parse(conversation.startedAt))
      .filter(Number.isFinite);
    setConversationStarts((current) => mergeConversationStarts(current, discovered));
  }, [conversations]);

  useEffect(() => {
    if (
      activeConversation
      && !conversations.some(
        (conversation) => conversation.id === selectedConversationId,
      )
    ) {
      setSelectedConversationId(activeConversation.id);
    }
  }, [activeConversation, conversations, selectedConversationId]);

  const createConversation = useCallback(() => {
    if (activeConversation && !activeConversation.subtitles.length) {
      setSelectedConversationId(activeConversation.id);
      return;
    }
    const latestSubtitleAt = subtitles.reduce(
      (latest, subtitle) => (
        Math.max(latest, Date.parse(subtitle.created_at) || 0)
      ),
      0,
    );
    const latestBoundary = (
      conversationStarts[conversationStarts.length - 1] ?? 0
    );
    const startedAt = Math.max(
      Date.now(),
      latestSubtitleAt + 1,
      latestBoundary + 1,
    );
    setConversationStarts((current) => (
      [...current, startedAt]
        .sort((left, right) => left - right)
        .slice(-50)
    ));
    setSelectedConversationId(conversationId(startedAt));
  }, [activeConversation, conversationStarts, subtitles]);

  const renameConversation = useCallback((id: string, value: string) => {
    const title = normalizeConversationTitle(value);
    if (!title) return;
    setCustomizations((current) => ({
      ...current,
      [id]: { ...current[id], title },
    }));
  }, []);

  const setConversationIcon = useCallback((id: string, icon: ConversationIcon | null) => {
    setCustomizations((current) => {
      const next = { ...current[id], icon: icon ?? undefined };
      if (!next.title && !next.icon) {
        const { [id]: _removed, ...rest } = current;
        return rest;
      }
      return { ...current, [id]: next };
    });
  }, []);

  const resetConversationCustomization = useCallback((id: string) => {
    setCustomizations((current) => {
      const { [id]: _removed, ...rest } = current;
      return rest;
    });
  }, []);

  const onLiveScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    if (page !== "live") return;
    const scrollRegion = event.currentTarget;
    setFollowingLiveSubtitles((current) => shouldFollowLiveScroll(current, {
      scrollTop: scrollRegion.scrollTop,
      previousScrollTop: previousLiveScrollTopRef.current,
      scrollHeight: scrollRegion.scrollHeight,
      clientHeight: scrollRegion.clientHeight,
    }));
    previousLiveScrollTopRef.current = scrollRegion.scrollTop;
  }, [page]);

  return {
    conversations,
    activeConversation,
    selectedConversation,
    sidebarOpen,
    setSidebarOpen,
    selectConversation: setSelectedConversationId,
    createConversation,
    renameConversation,
    setConversationIcon,
    resetConversationCustomization,
    liveScrollRef,
    followingLiveSubtitles,
    scrollLiveViewToBottom,
    onLiveScroll,
  };
}
