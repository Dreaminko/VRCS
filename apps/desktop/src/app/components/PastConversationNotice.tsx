import { Clock3 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ConversationSummary } from "../../conversations/conversations";
import { conversationTime } from "../app-utils";

export function PastConversationNotice({
  conversation,
  locale,
  onReturnCurrent,
}: {
  conversation: ConversationSummary;
  locale: string;
  onReturnCurrent: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="conversation-history-notice">
      <Clock3 size={15} />
      <span>
        {t("conversations.viewingPast", {
          time: conversationTime(
            conversation.startedAt,
            locale,
            t("date.today"),
            t("date.yesterday"),
          ),
        })}
      </span>
      <button type="button" onClick={onReturnCurrent}>
        {t("conversations.returnCurrent")}
      </button>
    </div>
  );
}
