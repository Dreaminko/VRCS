import { useTranslation } from "react-i18next";
import { MessageSquareText, PanelLeftClose, PanelLeftOpen, Plus } from "lucide-react";

import type { SubtitleConversation } from "../conversations";
import { conversationTime } from "../app-utils";

export function ConversationSidebar({ open, conversations, activeId, selectedId, onToggle, onNew, onSelect }: {
  open: boolean;
  conversations: SubtitleConversation[];
  activeId?: string;
  selectedId?: string;
  onToggle: () => void;
  onNew: () => void;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const active = conversations.find((conversation) => conversation.id === activeId);
  const history = conversations.filter((conversation) => conversation.id !== activeId);

  if (!open) {
    return (
      <aside className="conversation-sidebar conversation-sidebar-collapsed" aria-label={t("conversations.sidebar")}>
        <button className="sidebar-icon-button" type="button" aria-label={t("conversations.expandSidebar")} aria-expanded="false" onClick={onToggle}><PanelLeftOpen size={19} /></button>
        <button className="sidebar-icon-button sidebar-new-icon" type="button" aria-label={t("conversations.create")} onClick={onNew}><Plus size={20} /></button>
        {active && <button className={`sidebar-icon-button sidebar-current-icon ${selectedId === active.id ? "active" : ""}`} type="button" aria-label={t("conversations.viewCurrent")} onClick={() => onSelect(active.id)}><MessageSquareText size={19} /></button>}
      </aside>
    );
  }

  return (
    <aside className="conversation-sidebar" aria-label={t("conversations.sidebar")}>
      <div className="conversation-sidebar-header">
        <span>{t("conversations.title")}</span>
        <button className="sidebar-icon-button" type="button" aria-label={t("conversations.collapseSidebar")} aria-expanded="true" onClick={onToggle}><PanelLeftClose size={19} /></button>
      </div>
      <button className="new-conversation-button" type="button" onClick={onNew}><Plus size={18} />{t("conversations.create")}</button>
      <div className="conversation-sidebar-list">
        {active && (
          <section className="conversation-group" aria-labelledby="current-conversation-heading">
            <h2 id="current-conversation-heading">{t("conversations.current")}</h2>
            <ConversationButton conversation={active} active selected={selectedId === active.id} onSelect={onSelect} />
          </section>
        )}
        <section className="conversation-group" aria-labelledby="recent-conversations-heading">
          <h2 id="recent-conversations-heading">{t("conversations.previous")}</h2>
          {history.length ? history.map((conversation) => (
            <ConversationButton key={conversation.id} conversation={conversation} selected={selectedId === conversation.id} onSelect={onSelect} />
          )) : <p className="conversation-list-empty">{t("conversations.empty")}</p>}
        </section>
      </div>
    </aside>
  );
}

function ConversationButton({ conversation, active = false, selected, onSelect }: {
  conversation: SubtitleConversation;
  active?: boolean;
  selected: boolean;
  onSelect: (id: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  return (
    <button
      className={`conversation-button ${selected ? "selected" : ""}`}
      type="button"
      aria-current={selected ? "true" : undefined}
      onClick={() => onSelect(conversation.id)}
    >
      <span className="conversation-button-title"><MessageSquareText size={16} /><strong>{conversation.title}</strong>{active && <i aria-label={t("conversations.current")} />}</span>
      <span className="conversation-button-meta">
        <time>{conversationTime(conversation.startedAt, locale, t("date.today"), t("date.yesterday"))}</time>
        <span>{t("conversations.subtitleCount", { count: conversation.subtitles.length })}</span>
      </span>
    </button>
  );
}
