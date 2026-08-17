import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Page } from "../app-types";
import { normalizeConversationTitle } from "../conversation-state";
import { normalizeConversationSidebarWidth } from "../conversation-sidebar-width";
import {
  conversationsFromCatalog,
  type ConversationIcon,
} from "../conversations";
import type { ConversationCatalogEvent } from "../subtitle-stream";
import type { Subtitle } from "../types";
import { useConversationCatalog } from "./useConversationCatalog";
import { useLiveConversationScroll } from "./useLiveConversationScroll";

const SIDEBAR_OPEN_KEY = "vrcs.conversation-sidebar-open";
const SIDEBAR_WIDTH_KEY = "vrcs.conversation-sidebar-width";

function initialSidebarWidth(): number {
  return normalizeConversationSidebarWidth(localStorage.getItem(SIDEBAR_WIDTH_KEY));
}

export function useConversationWorkspace({
  coreReady,
  openedConversationId,
  subtitles,
  conversationCatalogEvent,
  openConversation,
  page,
  running,
  hasOlderSubtitles,
  loadingConversationSubtitles,
  reportError,
  clearErrorFrom,
}: {
  coreReady: boolean;
  openedConversationId: string | null;
  subtitles: Subtitle[];
  conversationCatalogEvent: ConversationCatalogEvent | null;
  openConversation: (conversationId: string | null) => Promise<void>;
  page: Page;
  running: boolean;
  hasOlderSubtitles: boolean;
  loadingConversationSubtitles: boolean;
  reportError: (reason: unknown, fallbackKey: string, source?: string) => void;
  clearErrorFrom: (source: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const [sidebarOpen, setSidebarOpen] = useState(
    () => localStorage.getItem(SIDEBAR_OPEN_KEY) !== "false",
  );
  const [sidebarWidth, setSidebarWidth] = useState(initialSidebarWidth);
  const catalogState = useConversationCatalog({
    coreReady,
    conversationCatalogEvent,
    openConversation,
    reportError,
    clearErrorFrom,
  });
  const conversations = useMemo(
    () => conversationsFromCatalog(catalogState.catalog, {
      untitled: t("conversations.untitled"),
      newConversation: t("conversations.new"),
    }),
    [catalogState.catalog, i18n.resolvedLanguage, t],
  );
  const activeConversation = conversations.find((conversation) => conversation.active)
    ?? conversations[0];
  const selectedConversation = conversations.find(
    (conversation) => conversation.id === catalogState.selectedConversationId,
  ) ?? activeConversation;
  const selectedSubtitles = selectedConversation?.id === openedConversationId
    ? subtitles
    : [];
  const selectedConversationHasOlder = Boolean(
    selectedConversation
    && selectedConversation.id === openedConversationId
    && hasOlderSubtitles,
  );
  const liveScroll = useLiveConversationScroll({
    page,
    running,
    activeConversationId: activeConversation?.id ?? null,
    selectedConversationId: selectedConversation?.id ?? null,
    openedConversationId,
    loadingConversationSubtitles,
    selectedConversationUpdatedAt: selectedConversation?.updatedAt ?? null,
  });

  useEffect(() => {
    localStorage.setItem(SIDEBAR_OPEN_KEY, String(sidebarOpen));
  }, [sidebarOpen]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(sidebarWidth));
    }, 120);
    return () => window.clearTimeout(timer);
  }, [sidebarWidth]);

  const renameConversation = (id: string, value: string) => {
    const title = normalizeConversationTitle(value);
    if (title) void catalogState.updateConversation(id, { custom_title: title });
  };

  const setConversationIcon = (id: string, icon: ConversationIcon | null) => {
    void catalogState.updateConversation(id, { icon });
  };

  const resetConversationCustomization = (id: string) => {
    void catalogState.updateConversation(id, {
      custom_title: null,
      icon: null,
    });
  };

  return {
    conversations,
    activeConversation,
    selectedConversation,
    selectedSubtitles,
    sidebarOpen,
    setSidebarOpen,
    sidebarWidth,
    setSidebarWidth,
    selectConversation: catalogState.selectConversation,
    createConversation: catalogState.createConversation,
    renameConversation,
    setConversationIcon,
    resetConversationCustomization,
    deleteConversation: catalogState.deleteConversation,
    liveScrollRef: liveScroll.liveScrollRef,
    followingLiveSubtitles: liveScroll.followingLiveSubtitles,
    selectedConversationHasOlder,
    scrollLiveViewToBottom: liveScroll.scrollLiveViewToBottom,
    onLiveScroll: liveScroll.onLiveScroll,
  };
}
