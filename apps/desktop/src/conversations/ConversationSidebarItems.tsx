import { useEffect, useRef } from "react";
import {
  Bookmark,
  Coffee,
  Gamepad2,
  Globe,
  GraduationCap,
  Heart,
  Headphones,
  Languages,
  MessageSquareText,
  Mic,
  Music,
  Sparkles,
  Star,
  Trophy,
  Users,
  Video,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { conversationTime } from "../app/app-utils";
import type { ConversationIcon, ConversationSummary } from "./conversations";

const ICONS: Record<ConversationIcon, LucideIcon> = {
  message: MessageSquareText,
  game: Gamepad2,
  headphones: Headphones,
  languages: Languages,
  study: GraduationCap,
  users: Users,
  bookmark: Bookmark,
  sparkles: Sparkles,
  mic: Mic,
  music: Music,
  video: Video,
  globe: Globe,
  heart: Heart,
  star: Star,
  coffee: Coffee,
  trophy: Trophy,
};

export function ConversationRow({
  conversation,
  active = false,
  selected,
  editing,
  editDraft,
  actionsOpen,
  onDraftChange,
  onCommitRename,
  onCancelRename,
  onSelect,
  onOpenActions,
}: {
  conversation: ConversationSummary;
  active?: boolean;
  selected: boolean;
  editing: boolean;
  editDraft: string;
  actionsOpen: boolean;
  onDraftChange: (value: string) => void;
  onCommitRename: () => void;
  onCancelRename: () => void;
  onSelect: (id: string) => void;
  onOpenActions: (
    conversation: ConversationSummary,
    target: HTMLButtonElement,
    clientX: number,
    clientY: number,
  ) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const inputRef = useRef<HTMLInputElement>(null);
  const cancelEditRef = useRef(false);

  useEffect(() => {
    if (!editing) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [editing]);

  return (
    <div className={`conversation-item ${selected ? "selected" : ""} ${editing ? "editing" : ""}`}>
      {editing ? (
        <div className="conversation-rename-row">
          <ConversationIconView icon={conversation.icon} size={16} />
          <input
            ref={inputRef}
            value={editDraft}
            maxLength={40}
            aria-label={t("conversations.editTitle")}
            aria-invalid={!editDraft.trim()}
            onChange={(event) => onDraftChange(event.target.value)}
            onBlur={() => {
              if (cancelEditRef.current) {
                cancelEditRef.current = false;
                return;
              }
              if (editDraft.trim()) onCommitRename();
              else onCancelRename();
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" && editDraft.trim()) {
                event.preventDefault();
                onCommitRename();
              } else if (event.key === "Escape") {
                event.preventDefault();
                cancelEditRef.current = true;
                onCancelRename();
              }
            }}
          />
        </div>
      ) : (
        <button
          className="conversation-button"
          type="button"
          aria-current={selected ? "true" : undefined}
          aria-haspopup="menu"
          aria-expanded={actionsOpen}
          onClick={() => onSelect(conversation.id)}
          onContextMenu={(event) => {
            event.preventDefault();
            const rect = event.currentTarget.getBoundingClientRect();
            const openedFromKeyboard = event.clientX === 0 && event.clientY === 0;
            onOpenActions(
              conversation,
              event.currentTarget,
              openedFromKeyboard ? rect.left + 24 : event.clientX,
              openedFromKeyboard ? rect.top + 24 : event.clientY,
            );
          }}
          onKeyDown={(event) => {
            if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
            event.preventDefault();
            const rect = event.currentTarget.getBoundingClientRect();
            onOpenActions(conversation, event.currentTarget, rect.left + 24, rect.top + 24);
          }}
        >
          <span className="conversation-button-title">
            <ConversationIconView icon={conversation.icon} size={16} />
            <strong>{conversation.title}</strong>
            {active && <i aria-label={t("conversations.current")} />}
          </span>
          <span className="conversation-button-meta">
            <time>{conversationTime(conversation.startedAt, locale, t("date.today"), t("date.yesterday"))}</time>
            <span>{t("conversations.subtitleCount", { count: conversation.subtitleCount })}</span>
          </span>
        </button>
      )}
    </div>
  );
}

export function RailConversationButton({
  conversation,
  active,
  selected,
  locale,
  onSelect,
  onShowTooltip,
  onHideTooltip,
}: {
  conversation: ConversationSummary;
  active: boolean;
  selected: boolean;
  locale: string;
  onSelect: (id: string) => void;
  onShowTooltip: (
    target: HTMLButtonElement,
    title: string,
    detail?: string,
    current?: boolean,
    keyboard?: boolean,
  ) => void;
  onHideTooltip: () => void;
}) {
  const { t } = useTranslation();
  const time = conversationTime(conversation.startedAt, locale, t("date.today"), t("date.yesterday"));
  return (
    <button
      className={`sidebar-icon-button sidebar-rail-conversation ${selected ? "active" : ""}`}
      type="button"
      aria-label={t("conversations.viewNamed", { title: conversation.title, time })}
      aria-current={selected ? "true" : undefined}
      onPointerEnter={(event) => onShowTooltip(event.currentTarget, conversation.title, time, active)}
      onPointerLeave={onHideTooltip}
      onFocus={(event) => onShowTooltip(event.currentTarget, conversation.title, time, active, event.currentTarget.matches(":focus-visible"))}
      onBlur={onHideTooltip}
      onClick={() => onSelect(conversation.id)}
    >
      <ConversationIconView icon={conversation.icon} size={18} />
      {active && <i aria-label={t("conversations.current")} />}
    </button>
  );
}

export function ConversationIconView({ icon, size }: {
  icon: ConversationIcon;
  size: number;
}) {
  const Icon = ICONS[icon];
  return <Icon size={size} />;
}
