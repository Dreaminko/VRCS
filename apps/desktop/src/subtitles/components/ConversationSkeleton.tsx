import { useTranslation } from "react-i18next";

const SKELETON_ROWS = [
  { mine: false, lines: ["72%", "48%"] },
  { mine: true, lines: ["56%"] },
  { mine: false, lines: ["68%", "40%"] },
  { mine: true, lines: ["62%", "34%"] },
] as const;

export function ConversationSkeleton() {
  const { t } = useTranslation();
  return (
    <div className="conversation-skeleton" role="status" aria-label={t("common.loading")}>
      {SKELETON_ROWS.map((row, index) => (
        <div
          key={index}
          className={`message-group ${row.mine ? "source-microphone" : "source-speaker"}`}
          aria-hidden="true"
        >
          <div className="conversation-skeleton-meta" />
          <div className="bubble conversation-skeleton-bubble">
            {row.lines.map((width, lineIndex) => (
              <span
                key={lineIndex}
                className="conversation-skeleton-line"
                style={{ width }}
              />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
