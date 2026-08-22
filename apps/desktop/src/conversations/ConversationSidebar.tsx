import { memo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { PanelLeftClose, Plus, Search, X } from "lucide-react";
import type { RefObject } from "react";
import type { ConversationIcon, ConversationSummary } from "./conversations";
import {
  DEFAULT_CONVERSATION_SIDEBAR_WIDTH,
  MAX_CONVERSATION_SIDEBAR_WIDTH,
  MIN_CONVERSATION_SIDEBAR_WIDTH,
  normalizeConversationSidebarWidth,
} from "./conversation-sidebar-width";
import { CollapsedConversationSidebar } from "./CollapsedConversationSidebar";
import {
  ConversationActionsPopover,
  conversationActionsPosition,
  type ConversationActionsPosition,
} from "./ConversationActionsPopover";
import { ConversationRow } from "./ConversationSidebarItems";
import { useConversationSidebarResize } from "./useConversationSidebarResize";
import type { SubtitleSearchHit } from "../subtitles/types";
import { ConversationSearchResults } from "./ConversationSearchResults";

type SidebarProps = {
  open: boolean;
  conversations: ConversationSummary[];
  activeId?: string;
  selectedId?: string;
  width: number;
  onWidthChange: (width: number) => void;
  onResizeStateChange: (resizing: boolean) => void;
  onToggle: () => void;
  onNew: () => void;
  onSelect: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onIconChange: (id: string, icon: ConversationIcon | null) => void;
  onResetCustomization: (id: string) => void;
  onDelete: (id: string) => Promise<void>;
  onSelectAt: (conversationId: string, subtitleId: number) => void;
  onFocusSearch: () => void;
  search: {
    query: string;
    setQuery: (query: string) => void;
    items: SubtitleSearchHit[];
    loading: boolean;
    loadingMore: boolean;
    hasMore: boolean;
    failed: boolean;
    searchable: boolean;
    active: boolean;
    clear: () => void;
    loadMore: () => Promise<void>;
    inputRef: RefObject<HTMLInputElement | null>;
  };
};


export const ConversationSidebar = memo(function ConversationSidebar({
  open,
  conversations,
  activeId,
  selectedId,
  width,
  onWidthChange,
  onResizeStateChange,
  onToggle,
  onNew,
  onSelect,
  onRename,
  onIconChange,
  onResetCustomization,
  onDelete,
  onSelectAt,
  onFocusSearch,
  search,
}: SidebarProps) {
  const { t } = useTranslation();
  const active = conversations.find((conversation) => conversation.id === activeId);
  const history = conversations.filter((conversation) => conversation.id !== activeId);
  const [actionsId, setActionsId] = useState<string | null>(null);
  const [actionsPopoverPosition, setActionsPopoverPosition] = useState<ConversationActionsPosition | null>(null);
  const [choosingIcon, setChoosingIcon] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const actionsTriggerRef = useRef<HTMLButtonElement>(null);

  const closeActions = (restoreFocus = false) => {
    setActionsId(null);
    setChoosingIcon(false);
    if (restoreFocus) window.requestAnimationFrame(() => actionsTriggerRef.current?.focus());
  };

  const resize = useConversationSidebarResize({
    width,
    onWidthChange,
    onResizeStateChange,
    onBeforeStart: () => closeActions(),
  });

  const handleToggle = () => {
    if (resize.resizing) resize.finish();
    closeActions();
    setEditingId(null);
    onToggle();
  };

  const selectConversation = (id: string) => {
    closeActions();
    onSelect(id);
  };

  const openActions = (
    conversation: ConversationSummary,
    target: HTMLButtonElement,
    clientX: number,
    clientY: number,
  ) => {
    actionsTriggerRef.current = target;
    setActionsPopoverPosition(conversationActionsPosition(clientX, clientY));
    setChoosingIcon(false);
    setActionsId(conversation.id);
  };

  const startRename = (conversation: ConversationSummary) => {
    setEditDraft(conversation.title);
    setEditingId(conversation.id);
    closeActions();
  };

  const menuConversation = conversations.find((conversation) => conversation.id === actionsId);

  return (
    <aside
      className={[
        "conversation-sidebar",
        open ? "" : "conversation-sidebar-collapsed",
        resize.resizing ? "conversation-sidebar-resizing" : "",
      ].filter(Boolean).join(" ")}
      aria-label={t("conversations.sidebar")}
    >
      {!open ? (
        <CollapsedConversationSidebar
          conversations={conversations}
          activeId={activeId}
          selectedId={selectedId}
          onToggle={handleToggle}
          onNew={onNew}
          onSelect={onSelect}
          onSearch={onFocusSearch}
        />
      ) : <>
      <div
        className="conversation-sidebar-resize-handle"
        role="separator"
        aria-label={t("conversations.sidebar")}
        aria-orientation="vertical"
        aria-valuemin={MIN_CONVERSATION_SIDEBAR_WIDTH}
        aria-valuemax={MAX_CONVERSATION_SIDEBAR_WIDTH}
        aria-valuenow={width}
        aria-valuetext={`${width}px`}
        tabIndex={0}
        onPointerDown={resize.start}
        onPointerMove={resize.resize}
        onPointerUp={resize.stop}
        onPointerCancel={resize.stop}
        onLostPointerCapture={resize.finish}
        onDoubleClick={() => onWidthChange(normalizeConversationSidebarWidth(
          DEFAULT_CONVERSATION_SIDEBAR_WIDTH,
        ))}
        onKeyDown={resize.resizeWithKeyboard}
      />
      <div className="conversation-sidebar-header">
        <span>{t("conversations.title")}</span>
        <button className="sidebar-icon-button" type="button" aria-label={t("conversations.collapseSidebar")} aria-expanded="true" onClick={handleToggle}><PanelLeftClose size={19} /></button>
      </div>
      <button className="new-conversation-button" type="button" onClick={onNew}><Plus size={18} /><span>{t("conversations.create")}</span></button>
      <div className="conversation-search-field">
        <Search size={15} aria-hidden="true" />
        <input
          ref={search.inputRef}
          type="search"
          value={search.query}
          aria-label={t("conversations.search")}
          placeholder={t("conversations.searchPlaceholder")}
          onChange={(event) => search.setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.preventDefault();
            search.clear();
          }}
        />
        {search.active && (
          <button type="button" aria-label={t("conversations.clearSearch")} onClick={search.clear}>
            <X size={14} />
          </button>
        )}
      </div>
      <div className={`conversation-sidebar-list ${search.active ? "search-active" : ""}`} onScroll={() => closeActions()}>
        {search.active ? (
          <ConversationSearchResults
            query={search.query}
            items={search.items}
            conversations={conversations}
            loading={search.loading}
            loadingMore={search.loadingMore}
            hasMore={search.hasMore}
            failed={search.failed}
            searchable={search.searchable}
            onLoadMore={search.loadMore}
            onSelect={onSelectAt}
          />
        ) : <>
        {active && (
          <section className="conversation-group" aria-labelledby="current-conversation-heading">
            <h2 id="current-conversation-heading">{t("conversations.current")}</h2>
            <ConversationRow
              conversation={active}
              active
              selected={selectedId === active.id}
              editing={editingId === active.id}
              editDraft={editDraft}
              actionsOpen={actionsId === active.id}
              onDraftChange={setEditDraft}
              onCommitRename={() => {
                onRename(active.id, editDraft);
                setEditingId(null);
              }}
              onCancelRename={() => setEditingId(null)}
              onSelect={selectConversation}
              onOpenActions={openActions}
            />
          </section>
        )}
        <section className="conversation-group" aria-labelledby="recent-conversations-heading">
          <h2 id="recent-conversations-heading">{t("conversations.previous")}</h2>
          {history.length ? history.map((conversation) => (
            <ConversationRow
              key={conversation.id}
              conversation={conversation}
              selected={selectedId === conversation.id}
              editing={editingId === conversation.id}
              editDraft={editDraft}
              actionsOpen={actionsId === conversation.id}
              onDraftChange={setEditDraft}
              onCommitRename={() => {
                onRename(conversation.id, editDraft);
                setEditingId(null);
              }}
              onCancelRename={() => setEditingId(null)}
              onSelect={selectConversation}
              onOpenActions={openActions}
            />
          )) : <p className="conversation-list-empty">{t("conversations.empty")}</p>}
        </section>
        </>}
      </div>
      {menuConversation && actionsPopoverPosition && (
        <ConversationActionsPopover
          conversation={menuConversation}
          position={actionsPopoverPosition}
          choosingIcon={choosingIcon}
          onChooseIcon={(icon) => {
            onIconChange(menuConversation.id, icon);
            closeActions();
          }}
          onStartRename={() => startRename(menuConversation)}
          onShowIconPicker={() => setChoosingIcon(true)}
          onResetCustomization={() => {
            onResetCustomization(menuConversation.id);
            closeActions();
          }}
          onDelete={() => {
            closeActions();
            void onDelete(menuConversation.id);
          }}
          onClose={closeActions}
        />
      )}
      </>}
    </aside>
  );
});
