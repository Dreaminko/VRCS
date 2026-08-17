import { useCallback, useEffect, useRef, useState } from "react";

import { coreApi } from "../api";
import {
  activeConversationId,
  catalogAfterRequest,
  catalogConversation,
  selectedConversationIdForCatalog,
} from "../conversation-state";
import type {
  ConversationCatalog,
  ConversationIcon,
} from "../conversations";
import type { ConversationCatalogEvent } from "../subtitle-stream";

type SelectionMode = "active" | "preserve";

export function useConversationCatalog({
  coreReady,
  conversationCatalogEvent,
  openConversation,
  reportError,
  clearErrorFrom,
}: {
  coreReady: boolean;
  conversationCatalogEvent: ConversationCatalogEvent | null;
  openConversation: (conversationId: string | null) => Promise<void>;
  reportError: (reason: unknown, fallbackKey: string, source?: string) => void;
  clearErrorFrom: (source: string) => void;
}) {
  const [catalog, setCatalog] = useState<ConversationCatalog | null>(null);
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const latestCatalogEventRef = useRef(conversationCatalogEvent);
  const appliedCatalogSequenceRef = useRef(0);
  const selectedConversationIdRef = useRef(selectedConversationId);
  latestCatalogEventRef.current = conversationCatalogEvent;
  selectedConversationIdRef.current = selectedConversationId;

  const applyCatalog = useCallback(async (
    nextCatalog: ConversationCatalog,
    selectionMode: SelectionMode,
  ) => {
    const currentId = selectedConversationIdRef.current;
    const nextId = selectionMode === "active"
      ? activeConversationId(nextCatalog)
      : selectedConversationIdForCatalog(nextCatalog, currentId);
    setCatalog(nextCatalog);
    if (nextId === currentId) return;
    selectedConversationIdRef.current = nextId;
    setSelectedConversationId(nextId);
    await openConversation(nextId);
  }, [openConversation]);

  const authoritativeResponse = useCallback((
    response: ConversationCatalog,
    requestStartSequence: number,
  ): ConversationCatalog => {
    const latestEvent = latestCatalogEventRef.current;
    if (latestEvent && latestEvent.sequence > requestStartSequence) {
      appliedCatalogSequenceRef.current = Math.max(
        appliedCatalogSequenceRef.current,
        latestEvent.sequence,
      );
    }
    return catalogAfterRequest(response, requestStartSequence, latestEvent);
  }, []);

  useEffect(() => {
    if (!coreReady) {
      setCatalog(null);
      setSelectedConversationId(null);
      selectedConversationIdRef.current = null;
      appliedCatalogSequenceRef.current = latestCatalogEventRef.current?.sequence ?? 0;
      return;
    }

    let cancelled = false;
    const requestStartSequence = latestCatalogEventRef.current?.sequence ?? 0;
    const loadCatalog = async () => {
      try {
        const response = await coreApi.conversations();
        if (cancelled) return;
        await applyCatalog(
          authoritativeResponse(response, requestStartSequence),
          "preserve",
        );
        clearErrorFrom("conversation-catalog");
      } catch (reason) {
        if (!cancelled) {
          reportError(reason, "errors.core.connect", "conversation-catalog");
        }
      }
    };
    void loadCatalog();
    return () => {
      cancelled = true;
    };
  }, [applyCatalog, authoritativeResponse, clearErrorFrom, coreReady, reportError]);

  useEffect(() => {
    if (
      !coreReady
      || conversationCatalogEvent === null
      || conversationCatalogEvent.sequence <= appliedCatalogSequenceRef.current
    ) return;
    appliedCatalogSequenceRef.current = conversationCatalogEvent.sequence;
    void applyCatalog(conversationCatalogEvent.catalog, "preserve");
    clearErrorFrom("conversation-catalog");
  }, [applyCatalog, clearErrorFrom, conversationCatalogEvent, coreReady]);

  const selectConversation = useCallback((id: string) => {
    if (!catalogConversation(catalog, id) || id === selectedConversationIdRef.current) return;
    selectedConversationIdRef.current = id;
    setSelectedConversationId(id);
    void openConversation(id);
  }, [catalog, openConversation]);

  const createConversation = useCallback(async (): Promise<boolean> => {
    const requestStartSequence = latestCatalogEventRef.current?.sequence ?? 0;
    try {
      const response = await coreApi.createConversation();
      await applyCatalog(
        authoritativeResponse(response, requestStartSequence),
        "active",
      );
      clearErrorFrom("conversation-catalog");
      return true;
    } catch (reason) {
      reportError(reason, "errors.operation", "conversation-catalog");
      return false;
    }
  }, [applyCatalog, authoritativeResponse, clearErrorFrom, reportError]);

  const updateConversation = useCallback(async (
    id: string,
    input: { custom_title?: string | null; icon?: ConversationIcon | null },
  ) => {
    if (!catalogConversation(catalog, id)) return;
    const requestStartSequence = latestCatalogEventRef.current?.sequence ?? 0;
    try {
      const response = await coreApi.updateConversation(id, input);
      await applyCatalog(
        authoritativeResponse(response, requestStartSequence),
        "preserve",
      );
      clearErrorFrom("conversation-catalog");
    } catch (reason) {
      reportError(reason, "errors.operation", "conversation-catalog");
    }
  }, [applyCatalog, authoritativeResponse, catalog, clearErrorFrom, reportError]);

  const deleteConversation = useCallback(async (id: string) => {
    if (!catalogConversation(catalog, id)) return;
    const requestStartSequence = latestCatalogEventRef.current?.sequence ?? 0;
    try {
      const response = await coreApi.deleteConversation(id);
      await applyCatalog(
        authoritativeResponse(response, requestStartSequence),
        "preserve",
      );
      clearErrorFrom("conversation-catalog");
    } catch (reason) {
      reportError(reason, "errors.subtitles.delete_failed", "conversation-catalog");
    }
  }, [applyCatalog, authoritativeResponse, catalog, clearErrorFrom, reportError]);

  return {
    catalog,
    selectedConversationId,
    selectConversation,
    createConversation,
    updateConversation,
    deleteConversation,
  };
}
