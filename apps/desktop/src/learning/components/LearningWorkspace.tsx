import { memo, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { prefersReducedMotion } from "../../app/app-motion";
import type { LookupOrigin } from "../../app/app-types";
import type { ConversationSummary } from "../../conversations/conversations";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import type { LearningItem } from "../types";
import type { Subtitle } from "../../subtitles/types";
import { LearningInbox } from "./LearningInbox";
import { LearningSourceBrowser } from "./LearningSourceBrowser";

export const LearningWorkspace = memo(function LearningWorkspace({
  conversation,
  conversations = [],
  activeConversationId,
  onSelectConversation,
  subtitles,
  workspace,
  ankiEnabled,
  ankiAvailable,
  onOpenAiSettings,
  onSelect,
  onTranslate,
  translatingSubtitleIds = [],
  hasOlder,
  loading,
  loadingOlder,
  onLoadOlder,
  targetViewKey,
  committedViewKey,
}: {
  conversation: ConversationSummary | undefined;
  conversations?: ConversationSummary[];
  activeConversationId?: string;
  onSelectConversation?: (id: string) => void;
  subtitles: Subtitle[];
  workspace: LearningWorkspaceController;
  ankiEnabled: boolean;
  ankiAvailable: boolean;
  onOpenAiSettings: () => void;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translatingSubtitleIds?: number[];
  hasOlder: boolean;
  loading: boolean;
  loadingOlder: boolean;
  onLoadOlder: () => Promise<void>;
  targetViewKey?: string | null;
  committedViewKey?: string | null;
}) {
  const { t } = useTranslation();
  const [materialsMounted, setMaterialsMounted] = useState(false);
  const [showMaterials, setShowMaterials] = useState(false);
  const [transition, setTransition] = useState<"to-materials" | "to-inbox" | null>(null);
  const [detailOpen, setDetailOpen] = useState(workspace.selectedId !== null);
  const [collectedId, setCollectedId] = useState<number | null>(null);
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;

  useEffect(() => {
    if (!transition) return;
    const timer = globalThis.setTimeout(() => setTransition(null), 185);
    return () => globalThis.clearTimeout(timer);
  }, [transition]);

  const openMaterials = useCallback(() => {
    setMaterialsMounted(true);
    setTransition(prefersReducedMotion() ? null : "to-materials");
    setShowMaterials(true);
  }, []);

  const closeMaterials = useCallback(() => {
    setTransition(prefersReducedMotion() ? null : "to-inbox");
    setShowMaterials(false);
  }, []);

  const openItemInInbox = useCallback((itemId: number) => {
    const currentWorkspace = workspaceRef.current;
    if (currentWorkspace.statusFilter !== "all" && currentWorkspace.statusFilter !== "collected") {
      currentWorkspace.setStatusFilter("all");
    }
    currentWorkspace.setSelectedId(itemId);
    setDetailOpen(true);
    closeMaterials();
  }, [closeMaterials]);

  const handleOpenCollected = useCallback(() => {
    if (collectedId !== null) openItemInInbox(collectedId);
  }, [collectedId, openItemInInbox]);

  const handleDismissCollected = useCallback(() => {
    setCollectedId(null);
  }, []);

  const handleOpenItem = useCallback((item: LearningItem) => {
    openItemInInbox(item.id);
  }, [openItemInInbox]);

  const handleCollected = useCallback((item: LearningItem) => {
    setCollectedId(item.id);
    const currentWorkspace = workspaceRef.current;
    if (currentWorkspace.statusFilter !== "all" && currentWorkspace.statusFilter !== "collected") {
      currentWorkspace.setStatusFilter("all");
    }
  }, []);

  const finishTransition = useCallback((expected: "to-materials" | "to-inbox") => {
    setTransition((current) => (current === expected ? null : current));
  }, []);

  const inboxTransitionState = transition === "to-materials"
    ? "exit-to-materials"
    : transition === "to-inbox"
      ? "enter-from-materials"
      : undefined;
  const materialsTransitionState = transition === "to-materials"
    ? "enter-from-inbox"
    : transition === "to-inbox"
      ? "exit-to-inbox"
      : undefined;

  return (
    <section className="learning-workspace" aria-label={t("learning.title")}>
      <div
        className="learning-workspace-view"
        hidden={showMaterials && transition === null}
        inert={showMaterials}
        aria-hidden={showMaterials ? "true" : undefined}
        data-transition-state={inboxTransitionState}
        onAnimationEnd={(event) => {
          if (event.target === event.currentTarget) finishTransition("to-inbox");
        }}
      >
        <LearningInbox
          workspace={workspace}
          ankiEnabled={ankiEnabled}
          ankiAvailable={ankiAvailable}
          onOpenAiSettings={onOpenAiSettings}
          detailOpen={detailOpen}
          onDetailOpen={setDetailOpen}
          visible={!showMaterials}
          onOpenMaterials={openMaterials}
        />
      </div>
      <div
        className="learning-workspace-view learning-materials-view"
        hidden={!showMaterials && transition === null}
        inert={!showMaterials}
        aria-hidden={!showMaterials ? "true" : undefined}
        data-transition-state={materialsTransitionState}
        onAnimationEnd={(event) => {
          if (event.target === event.currentTarget) finishTransition("to-materials");
        }}
      >
        {materialsMounted && (
          <LearningSourceBrowser
            conversation={conversation}
            conversations={conversations}
            activeConversationId={activeConversationId}
            onSelectConversation={onSelectConversation}
            subtitles={subtitles}
            workspace={workspace}
            onSelect={onSelect}
            onTranslate={onTranslate}
            translatingSubtitleIds={translatingSubtitleIds}
            hasOlder={hasOlder}
            loading={loading}
            loadingOlder={loadingOlder}
            onLoadOlder={onLoadOlder}
            targetViewKey={targetViewKey}
            committedViewKey={committedViewKey}
            onBackToInbox={closeMaterials}
            collectedId={collectedId}
            onOpenCollected={handleOpenCollected}
            onDismissCollected={handleDismissCollected}
            onOpenItem={handleOpenItem}
            onCollected={handleCollected}
          />
        )}
      </div>
    </section>
  );
});
