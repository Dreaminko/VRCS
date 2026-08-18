import { memo, useState } from "react";
import { useTranslation } from "react-i18next";
import { BookOpenText, Inbox } from "lucide-react";

import type { LookupOrigin } from "../../app/app-types";
import type { ConversationSummary } from "../../conversations/conversations";
import type { LearningWorkspaceController } from "../hooks/useLearningWorkspace";
import type { Subtitle } from "../../types";
import { LearningInbox } from "./LearningInbox";
import { LearningSourceBrowser } from "./LearningSourceBrowser";

type LearningTab = "inbox" | "materials";

export const LearningWorkspace = memo(function LearningWorkspace({
  conversation,
  subtitles,
  workspace,
  ankiEnabled,
  onSelect,
  onTranslate,
  translatingSubtitleIds = [],
  hasOlder,
  loading,
  loadingOlder,
  onLoadOlder,
}: {
  conversation: ConversationSummary | undefined;
  subtitles: Subtitle[];
  workspace: LearningWorkspaceController;
  ankiEnabled: boolean;
  onSelect: (context: string, origin?: LookupOrigin) => Promise<void>;
  onTranslate?: (subtitleId: number) => void;
  translatingSubtitleIds?: number[];
  hasOlder: boolean;
  loading: boolean;
  loadingOlder: boolean;
  onLoadOlder: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<LearningTab>("inbox");

  return (
    <section className="learning-workspace" aria-label={t("learning.title")}>
      <header className="learning-workspace-header">
        <div><h1>{t("learning.title")}</h1><p>{t("learning.description")}</p></div>
        <div className="learning-tabs" role="tablist" aria-label={t("learning.tabs.label")}>
          <button type="button" role="tab" aria-selected={tab === "inbox"} className={tab === "inbox" ? "active" : ""} onClick={() => setTab("inbox")}><Inbox size={15} />{t("learning.tabs.inbox")}</button>
          <button type="button" role="tab" aria-selected={tab === "materials"} className={tab === "materials" ? "active" : ""} onClick={() => setTab("materials")}><BookOpenText size={15} />{t("learning.tabs.materials")}</button>
        </div>
      </header>
      {tab === "inbox" ? (
        <LearningInbox workspace={workspace} ankiEnabled={ankiEnabled} />
      ) : (
        <LearningSourceBrowser
          conversation={conversation}
          subtitles={subtitles}
          workspace={workspace}
          onSelect={onSelect}
          onTranslate={onTranslate}
          translatingSubtitleIds={translatingSubtitleIds}
          hasOlder={hasOlder}
          loading={loading}
          loadingOlder={loadingOlder}
          onLoadOlder={onLoadOlder}
          onCollected={() => {
            workspace.setStatusFilter("all");
            setTab("inbox");
          }}
        />
      )}
    </section>
  );
});
