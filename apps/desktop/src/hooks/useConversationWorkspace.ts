import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { UIEvent } from "react";
import { useTranslation } from "react-i18next";

import type { Page } from "../app-types";
import { conversationId, groupConversations } from "../conversations";
import { shouldFollowLiveScroll } from "../live-scroll";
import type { Subtitle } from "../types";

const CONVERSATION_STARTS_KEY = "vrcs.conversation-starts.v1";
const SIDEBAR_OPEN_KEY = "vrcs.conversation-sidebar-open";

function storedConversationStarts() {
  try {
    const value = JSON.parse(
      localStorage.getItem(CONVERSATION_STARTS_KEY) ?? "[]",
    ) as unknown;
    return Array.isArray(value)
      ? value
        .filter(
          (item): item is number => (
            typeof item === "number" && Number.isFinite(item)
          ),
        )
        .slice(-50)
      : [];
  } catch {
    return [];
  }
}

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
  const [conversationStarts, setConversationStarts] = useState(
    storedConversationStarts,
  );
  const [selectedConversationId, setSelectedConversationId] = useState<
    string | null
  >(null);
  const conversations = useMemo(
    () => groupConversations(subtitles, conversationStarts, openedAt, {
      untitled: t("conversations.untitled"),
      newConversation: t("conversations.new"),
    }),
    [conversationStarts, i18n.resolvedLanguage, openedAt, subtitles, t],
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
    localStorage.setItem(
      CONVERSATION_STARTS_KEY,
      JSON.stringify(conversationStarts),
    );
  }, [conversationStarts]);

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
    liveScrollRef,
    followingLiveSubtitles,
    scrollLiveViewToBottom,
    onLiveScroll,
  };
}
