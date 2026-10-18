import { memo, useEffect, useState } from "react";
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
}) {
  const { t } = useTranslation();
  const [showMaterials, setShowMaterials] = useState(false);
  const [transition, setTransition] = useState<"to-materials" | "to-inbox" | null>(null);
  const [detailOpen, setDetailOpen] = useState(workspace.selectedId !== null);
  const [collectedId, setCollectedId] = useState<number | null>(null);

  useEffect(() => {
    if (!transition) return;
    const timer = globalThis.setTimeout(() => setTransition(null), 185);
    return () => globalThis.clearTimeout(timer);
  }, [transition]);

  const openMaterials = () => {
    setTransition(prefersReducedMotion() ? null : "to-materials");
    setShowMaterials(true);
  };

  const closeMaterials = () => {
    setTransition(prefersReducedMotion() ? null : "to-inbox");
    setShowMaterials(false);
  };

  const openItemInInbox = (itemId: number) => {
    if (workspace.statusFilter !== "all" && workspace.statusFilter !== "collected") {
      workspace.setStatusFilter("all");
    }
    workspace.setSelectedId(itemId);
    setDetailOpen(true);
    closeMaterials();
  };

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
          if (event.target === event.currentTarget && transition === "to-inbox") {
            setTransition(null);
          }
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
          if (event.target === event.currentTarget && transition === "to-materials") {
            setTransition(null);
          }
        }}
      >
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
          onBackToInbox={closeMaterials}
          collectedId={collectedId}
          onOpenCollected={() => {
            if (collectedId !== null) openItemInInbox(collectedId);
          }}
          onDismissCollected={() => setCollectedId(null)}
          onOpenItem={(item: LearningItem) => openItemInInbox(item.id)}
          onCollected={(item) => {
            setCollectedId(item.id);
            if (workspace.statusFilter !== "all" && workspace.statusFilter !== "collected") {
              workspace.setStatusFilter("all");
            }
          }}
        />
      </div>
    </section>
  );
});
