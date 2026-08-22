import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { learningApi } from "../api";
import { localizedError } from "../../app/app-utils";
import {
  LEARNING_PAGE_SIZE,
  learningItemMatchesStatus,
  mergeLearningItemPages,
  normalizeLearningCardDraft,
} from "../../learning";
import type {
  LearningAnalysisInput,
  LearningCardDraft,
  LearningCardType,
  LearningItem,
  LearningItemStatus,
} from "../types";

export type LearningStatusFilter = LearningItemStatus | "all";
export type LearningBusyAction = "save" | "analysis" | "draft" | "export" | "status" | "delete";

export function useLearningInbox(ready: boolean) {
  const { t } = useTranslation();
  const [items, setItems] = useState<LearningItem[]>([]);
  const [statusFilter, setStatusFilterState] = useState<LearningStatusFilter>("all");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState("");
  const [itemErrors, setItemErrors] = useState<Record<number, string>>({});
  const [busyByItem, setBusyByItem] = useState<Record<number, LearningBusyAction | undefined>>({});
  const itemBusyRef = useRef(new Map<number, LearningBusyAction>());
  const listRequestRef = useRef(0);

  const selectedItem = useMemo(
    () => items.find((item) => item.id === selectedId) ?? null,
    [items, selectedId],
  );

  const setStatusFilter = useCallback((status: LearningStatusFilter) => {
    setStatusFilterState(status);
    setSelectedId(null);
  }, []);

  const loadItems = useCallback(async (append = false) => {
    const requestId = ++listRequestRef.current;
    if (append) setLoadingMore(true);
    else setLoading(true);
    setError("");
    try {
      const beforeId = append && items.length
        ? Math.min(...items.map((item) => item.id))
        : undefined;
      const response = await learningApi.learningItems({
        status: statusFilter === "all" ? undefined : statusFilter,
        limit: LEARNING_PAGE_SIZE,
        beforeId,
      });
      if (requestId !== listRequestRef.current) return;
      setItems((current) => append
        ? mergeLearningItemPages(current, response)
        : mergeLearningItemPages([], response));
      setHasMore(response.length >= LEARNING_PAGE_SIZE);
    } catch (reason) {
      if (requestId === listRequestRef.current) {
        setError(localizedError(reason, t, "errors.learning.load"));
      }
    } finally {
      if (requestId === listRequestRef.current) {
        setLoading(false);
        setLoadingMore(false);
      }
    }
  }, [items, statusFilter, t]);

  useEffect(() => {
    if (!ready) return;
    void loadItems(false);
  }, [ready, statusFilter]);

  useEffect(() => {
    if (selectedId !== null && items.some((item) => item.id === selectedId)) return;
    setSelectedId(items[0]?.id ?? null);
  }, [items, selectedId]);

  useEffect(() => () => {
    listRequestRef.current += 1;
  }, []);

  const applyItem = useCallback((item: LearningItem) => {
    setItems((current) => {
      if (!learningItemMatchesStatus(item, statusFilter)) {
        return current.filter((candidate) => candidate.id !== item.id);
      }
      return mergeLearningItemPages(current, [item]);
    });
    setSelectedId(item.id);
    setItemErrors((current) => removeRecordKey(current, item.id));
    return item;
  }, [statusFilter]);

  const runItemAction = useCallback(async (
    itemId: number,
    action: LearningBusyAction,
    request: () => Promise<LearningItem>,
    fallbackKey: string,
  ): Promise<LearningItem | null> => {
    if (itemBusyRef.current.has(itemId)) return null;
    itemBusyRef.current.set(itemId, action);
    setBusyByItem((current) => ({ ...current, [itemId]: action }));
    setItemErrors((current) => removeRecordKey(current, itemId));
    try {
      return applyItem(await request());
    } catch (reason) {
      setItemErrors((current) => ({
        ...current,
        [itemId]: localizedError(reason, t, fallbackKey),
      }));
      return null;
    } finally {
      itemBusyRef.current.delete(itemId);
      setBusyByItem((current) => removeRecordKey(current, itemId));
    }
  }, [applyItem, t]);

  const updateWorkingText = useCallback((itemId: number, workingText: string) => runItemAction(
    itemId,
    "save",
    () => learningApi.updateLearningItem(itemId, { working_text: workingText }),
    "errors.learning.save",
  ), [runItemAction]);

  const analyze = useCallback((itemId: number, input: LearningAnalysisInput) => runItemAction(
    itemId,
    "analysis",
    () => learningApi.analyzeLearningItem(itemId, input),
    "errors.learning.analysis",
  ), [runItemAction]);

  const generateDraft = useCallback((itemId: number, cardType: LearningCardType) => runItemAction(
    itemId,
    "draft",
    () => learningApi.generateLearningDraft(itemId, { card_type: cardType }),
    "errors.learning.draft",
  ), [runItemAction]);

  const saveDraft = useCallback((itemId: number, draft: LearningCardDraft) => runItemAction(
    itemId,
    "save",
    () => learningApi.updateLearningItem(itemId, { draft: normalizeLearningCardDraft(draft) }),
    "errors.learning.saveDraft",
  ), [runItemAction]);

  const exportItem = useCallback((itemId: number) => runItemAction(
    itemId,
    "export",
    () => learningApi.exportLearningItem(itemId),
    "errors.learning.export",
  ), [runItemAction]);

  const archiveItem = useCallback((itemId: number) => runItemAction(
    itemId,
    "status",
    () => learningApi.archiveLearningItem(itemId),
    "errors.learning.status",
  ), [runItemAction]);

  const restoreItem = useCallback((itemId: number) => runItemAction(
    itemId,
    "status",
    () => learningApi.restoreLearningItem(itemId),
    "errors.learning.status",
  ), [runItemAction]);

  const deleteItem = useCallback(async (itemId: number) => {
    if (itemBusyRef.current.has(itemId)) return { deleted: false, item: null };
    itemBusyRef.current.set(itemId, "delete");
    setBusyByItem((current) => ({ ...current, [itemId]: "delete" }));
    setItemErrors((current) => removeRecordKey(current, itemId));
    const item = items.find((candidate) => candidate.id === itemId) ?? null;
    try {
      await learningApi.deleteLearningItem(itemId);
      setItems((current) => current.filter((candidate) => candidate.id !== itemId));
      setSelectedId((current) => current === itemId ? null : current);
      return { deleted: true, item };
    } catch (reason) {
      setItemErrors((current) => ({
        ...current,
        [itemId]: localizedError(reason, t, "errors.learning.delete"),
      }));
      return { deleted: false, item };
    } finally {
      itemBusyRef.current.delete(itemId);
      setBusyByItem((current) => removeRecordKey(current, itemId));
    }
  }, [items, t]);

  return {
    items,
    selectedItem,
    selectedId,
    setSelectedId,
    statusFilter,
    setStatusFilter,
    loading,
    loadingMore,
    hasMore,
    reload: () => loadItems(false),
    loadMore: () => loadItems(true),
    error,
    clearError: () => setError(""),
    itemErrors,
    busyByItem,
    isItemBusy: (itemId: number) => itemBusyRef.current.has(itemId),
    applyItem,
    updateWorkingText,
    analyze,
    generateDraft,
    saveDraft,
    exportItem,
    archiveItem,
    restoreItem,
    deleteItem,
  };
}

function removeRecordKey<T>(record: Record<number, T>, key: number): Record<number, T> {
  const next = { ...record };
  delete next[key];
  return next;
}
