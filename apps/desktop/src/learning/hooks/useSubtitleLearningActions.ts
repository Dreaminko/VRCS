import { useCallback } from "react";

import type { SubtitleAnalysisOutcome } from "../../subtitle-actions";
import type { Subtitle } from "../../subtitles/types";
import { subtitleLearningKey, subtitleSelectionLearningKey } from "../../learning";
import type { LearningWorkspaceController } from "./useLearningWorkspace";

export function useSubtitleLearningActions({
  workspace,
  clearLookup,
  openLearningPage,
}: {
  workspace: LearningWorkspaceController;
  clearLookup: () => void;
  openLearningPage: () => void;
}) {
  const openLearningItem = useCallback((itemId: number) => {
    clearLookup();
    workspace.setStatusFilter("all");
    workspace.setSelectedId(itemId);
    openLearningPage();
  }, [clearLookup, openLearningPage, workspace.setSelectedId, workspace.setStatusFilter]);

  const openSubtitleLearning = useCallback(async (subtitle: Subtitle) => {
    const item = await workspace.collectSubtitle(subtitle);
    if (!item) return null;
    openLearningItem(item.id);
    return item;
  }, [openLearningItem, workspace.collectSubtitle]);

  const analyzeSubtitleSentence = useCallback(async (
    subtitle: Subtitle,
  ): Promise<SubtitleAnalysisOutcome | null> => {
    clearLookup();
    const item = await workspace.collectSubtitle(subtitle);
    if (!item) return null;
    if (
      item.status === "archived"
      || !workspace.preferences.profileId
      || !workspace.preferences.model.trim()
    ) {
      openLearningItem(item.id);
      return { status: "opened", itemId: item.id };
    }
    const analyzed = await workspace.analyze(item.id, "sentence_analysis");
    return analyzed?.analysis
      ? { status: "completed", itemId: analyzed.id, analysis: analyzed.analysis }
      : null;
  }, [
    clearLookup,
    openLearningItem,
    workspace.analyze,
    workspace.collectSubtitle,
    workspace.preferences.model,
    workspace.preferences.profileId,
  ]);

  const collectSubtitleSelection = useCallback((selection: Subtitle[]) => {
    const ids = subtitleIds(selection);
    return workspace.collectSubtitles(selection, ids, { mergeFragments: true });
  }, [workspace.collectSubtitles]);

  const openSubtitleSelectionLearning = useCallback(async (selection: Subtitle[]) => {
    const item = await collectSubtitleSelection(selection);
    if (!item) return null;
    openLearningItem(item.id);
    return item;
  }, [collectSubtitleSelection, openLearningItem]);

  const analyzeSubtitleSelection = useCallback(async (
    selection: Subtitle[],
  ): Promise<SubtitleAnalysisOutcome | null> => {
    clearLookup();
    const item = await collectSubtitleSelection(selection);
    if (!item) return null;
    if (
      item.status === "archived"
      || !workspace.preferences.profileId
      || !workspace.preferences.model.trim()
    ) {
      openLearningItem(item.id);
      return { status: "opened", itemId: item.id };
    }
    const analyzed = await workspace.analyze(item.id, "sentence_analysis");
    return analyzed?.analysis
      ? { status: "completed", itemId: analyzed.id, analysis: analyzed.analysis }
      : null;
  }, [
    clearLookup,
    collectSubtitleSelection,
    openLearningItem,
    workspace.analyze,
    workspace.preferences.model,
    workspace.preferences.profileId,
  ]);

  const isSubtitleLearningBusy = useCallback(
    (subtitle: Subtitle) => workspace.isCollecting(subtitleLearningKey(subtitle)),
    [workspace.isCollecting],
  );
  const isSubtitleLearningCaptured = useCallback(
    (subtitle: Subtitle) => workspace.isCaptured(subtitleLearningKey(subtitle)),
    [workspace.isCaptured],
  );
  const isSubtitleSelectionLearningBusy = useCallback(
    (selection: Subtitle[]) => workspace.isCollecting(
      subtitleSelectionLearningKey(subtitleIds(selection)),
    ),
    [workspace.isCollecting],
  );
  const isSubtitleSelectionLearningCaptured = useCallback(
    (selection: Subtitle[]) => workspace.isCaptured(
      subtitleSelectionLearningKey(subtitleIds(selection)),
    ),
    [workspace.isCaptured],
  );

  return {
    analyzeSubtitleSelection,
    analyzeSubtitleSentence,
    collectSubtitleSelection,
    isSubtitleLearningBusy,
    isSubtitleLearningCaptured,
    isSubtitleSelectionLearningBusy,
    isSubtitleSelectionLearningCaptured,
    openLearningItem,
    openSubtitleLearning,
    openSubtitleSelectionLearning,
  };
}

function subtitleIds(subtitles: Subtitle[]): number[] {
  return subtitles.flatMap((subtitle) => subtitle.id === null ? [] : [subtitle.id]);
}
