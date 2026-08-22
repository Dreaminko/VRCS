import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import type {
  LearningAnalysisFocus,
  LearningTaskType,
} from "../types";
import { useLearningAiConfiguration } from "./useLearningAiConfiguration";
import { useLearningCapture } from "./useLearningCapture";
import { useLearningInbox } from "./useLearningInbox";

export type { LearningPreferences } from "./useLearningAiConfiguration";
export type { LearningBusyAction, LearningStatusFilter } from "./useLearningInbox";

export function useLearningWorkspace(active: boolean, ready = active) {
  const { i18n } = useTranslation();
  const inbox = useLearningInbox(ready);
  const capture = useLearningCapture(ready, inbox.applyItem);
  const ai = useLearningAiConfiguration(active);

  const analyze = useCallback((
    itemId: number,
    taskType: LearningTaskType,
    focus?: LearningAnalysisFocus,
  ) => {
    if (!ai.preferences.profileId || !ai.preferences.model.trim()) {
      return Promise.resolve(null);
    }
    return inbox.analyze(itemId, {
      task_type: taskType,
      focus,
      profile_id: ai.preferences.profileId,
      model: ai.preferences.model.trim(),
      explanation_language: ai.preferences.explanationLanguage,
      level: ai.preferences.explanationLevel,
    });
  }, [ai.preferences, inbox.analyze]);

  const deleteItem = useCallback(async (itemId: number): Promise<boolean> => {
    const result = await inbox.deleteItem(itemId);
    if (!result.deleted) return false;
    await capture.refreshAfterDelete(result.item);
    return true;
  }, [capture.refreshAfterDelete, inbox.deleteItem]);

  return {
    items: inbox.items,
    selectedItem: inbox.selectedItem,
    selectedId: inbox.selectedId,
    setSelectedId: inbox.setSelectedId,
    statusFilter: inbox.statusFilter,
    setStatusFilter: inbox.setStatusFilter,
    loading: inbox.loading,
    loadingMore: inbox.loadingMore,
    hasMore: inbox.hasMore,
    reload: inbox.reload,
    loadMore: inbox.loadMore,
    error: inbox.error || capture.error || ai.error,
    clearError: () => {
      inbox.clearError();
      capture.clearError();
      ai.clearError();
    },
    itemErrors: inbox.itemErrors,
    busyByItem: inbox.busyByItem,
    isItemBusy: inbox.isItemBusy,
    isCollecting: capture.isCollecting,
    isCaptured: capture.isCaptured,
    captureBusyKeys: capture.captureBusyKeys,
    preferences: ai.preferences,
    collectSubtitle: capture.collectSubtitle,
    collectSubtitles: capture.collectSubtitles,
    collectLookup: capture.collectLookup,
    updateWorkingText: inbox.updateWorkingText,
    analyze,
    generateDraft: inbox.generateDraft,
    saveDraft: inbox.saveDraft,
    exportItem: inbox.exportItem,
    archiveItem: inbox.archiveItem,
    restoreItem: inbox.restoreItem,
    deleteItem,
    locale: i18n.resolvedLanguage ?? "en-US",
  };
}

export type LearningWorkspaceController = ReturnType<typeof useLearningWorkspace>;
