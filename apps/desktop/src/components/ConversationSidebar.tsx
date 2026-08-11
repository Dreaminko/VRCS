import { useEffect, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
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
  PanelLeftClose,
  PanelLeftOpen,
  Pencil,
  Plus,
  RotateCcw,
  Shapes,
  Sparkles,
  Star,
  Trophy,
  Users,
  Video,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { conversationTime } from "../app-utils";
import {
  CONVERSATION_ICON_KEYS,
  type ConversationIcon,
  type SubtitleConversation,
} from "../conversations";
import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../interface-scale";
import { useDismissibleLayer } from "../use-dismissible-layer";

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

type FloatingPosition = {
  top: number;
  left: number;
  side?: "above" | "below";
};

type SidebarTooltip = {
  top: number;
  left: number;
  maxWidth: number;
  title: string;
  detail?: string;
  current?: boolean;
  keyboard: boolean;
  visible: boolean;
};

const TOOLTIP_HOVER_DELAY_MS = 180;
const TOOLTIP_SWITCH_DELAY_MS = 70;
const TOOLTIP_EXIT_MS = 110;

type SidebarProps = {
  open: boolean;
  conversations: SubtitleConversation[];
  activeId?: string;
  selectedId?: string;
  onToggle: () => void;
  onNew: () => void;
  onSelect: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onIconChange: (id: string, icon: ConversationIcon | null) => void;
  onResetCustomization: (id: string) => void;
};

function actionsPosition(clientX: number, clientY: number): FloatingPosition {
  const scale = readAppliedInterfaceScaleFactor();
  const x = interfaceLayoutPixels(clientX, scale);
  const y = interfaceLayoutPixels(clientY, scale);
  const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
  const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
  const width = 186;
  const expectedHeight = 220;
  const gap = 4;
  const side = viewportHeight - y >= expectedHeight ? "below" : "above";
  return {
    top: side === "below" ? y + gap : y - gap,
    left: Math.max(8, Math.min(x + gap, viewportWidth - width - 8)),
    side,
  };
}

function movePopoverFocus(event: ReactKeyboardEvent<HTMLDivElement>) {
  const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
  let nextIndex = currentIndex;
  if (event.key === "ArrowDown" || event.key === "ArrowRight") nextIndex = (currentIndex + 1) % buttons.length;
  else if (event.key === "ArrowUp" || event.key === "ArrowLeft") nextIndex = (currentIndex - 1 + buttons.length) % buttons.length;
  else if (event.key === "Home") nextIndex = 0;
  else if (event.key === "End") nextIndex = buttons.length - 1;
  else return;
  event.preventDefault();
  buttons[nextIndex]?.focus();
}

export function ConversationSidebar({
  open,
  conversations,
  activeId,
  selectedId,
  onToggle,
  onNew,
  onSelect,
  onRename,
  onIconChange,
  onResetCustomization,
}: SidebarProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const active = conversations.find((conversation) => conversation.id === activeId);
  const history = conversations.filter((conversation) => conversation.id !== activeId);
  const [actionsId, setActionsId] = useState<string | null>(null);
  const [actionsPopoverPosition, setActionsPopoverPosition] = useState<FloatingPosition | null>(null);
  const [choosingIcon, setChoosingIcon] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [sidebarTooltip, setSidebarTooltip] = useState<SidebarTooltip | null>(null);
  const actionsLayerRef = useRef<HTMLDivElement>(null);
  const actionsTriggerRef = useRef<HTMLButtonElement>(null);
  const tooltipTimerRef = useRef<number | null>(null);
  const tooltipHideTimerRef = useRef<number | null>(null);
  const tooltipFrameRef = useRef<number | null>(null);

  const clearTooltipTimer = () => {
    if (tooltipTimerRef.current === null) return;
    window.clearTimeout(tooltipTimerRef.current);
    tooltipTimerRef.current = null;
  };

  const clearTooltipHideTimer = () => {
    if (tooltipHideTimerRef.current === null) return;
    window.clearTimeout(tooltipHideTimerRef.current);
    tooltipHideTimerRef.current = null;
  };

  const clearTooltipFrame = () => {
    if (tooltipFrameRef.current === null) return;
    window.cancelAnimationFrame(tooltipFrameRef.current);
    tooltipFrameRef.current = null;
  };

  const hideSidebarTooltip = () => {
    clearTooltipTimer();
    clearTooltipFrame();
    clearTooltipHideTimer();
    setSidebarTooltip((tooltip) => tooltip ? { ...tooltip, visible: false } : null);
    tooltipHideTimerRef.current = window.setTimeout(() => {
      setSidebarTooltip(null);
      tooltipHideTimerRef.current = null;
    }, TOOLTIP_EXIT_MS);
  };

  const showSidebarTooltip = (
    target: HTMLButtonElement,
    title: string,
    detail?: string,
    current = false,
    keyboard = false,
  ) => {
    clearTooltipTimer();
    clearTooltipHideTimer();
    clearTooltipFrame();
    const reveal = () => {
      const rect = target.getBoundingClientRect();
      const scale = readAppliedInterfaceScaleFactor();
      const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
      const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
      const left = interfaceLayoutPixels(rect.right, scale) + 10;
      const centerY = interfaceLayoutPixels(rect.top + rect.height / 2, scale);
      const nextTooltip: SidebarTooltip = {
        top: Math.max(30, Math.min(centerY, viewportHeight - 30)),
        left,
        maxWidth: Math.max(140, Math.min(240, viewportWidth - left - 12)),
        title,
        detail,
        current,
        keyboard,
        visible: keyboard,
      };
      setSidebarTooltip(nextTooltip);
      if (!keyboard) {
        tooltipFrameRef.current = window.requestAnimationFrame(() => {
          setSidebarTooltip((tooltip) => tooltip ? { ...tooltip, visible: true } : null);
          tooltipFrameRef.current = null;
        });
      }
      tooltipTimerRef.current = null;
    };
    if (keyboard) reveal();
    else tooltipTimerRef.current = window.setTimeout(
      reveal,
      sidebarTooltip ? TOOLTIP_SWITCH_DELAY_MS : TOOLTIP_HOVER_DELAY_MS,
    );
  };

  const closeActions = (restoreFocus = false) => {
    setActionsId(null);
    setChoosingIcon(false);
    if (restoreFocus) window.requestAnimationFrame(() => actionsTriggerRef.current?.focus());
  };
  useDismissibleLayer(Boolean(actionsId), actionsLayerRef, () => closeActions(true));

  useEffect(() => {
    if (!actionsId) return;
    window.requestAnimationFrame(() => {
      actionsLayerRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    });
  }, [actionsId, choosingIcon]);

  useEffect(() => () => {
    clearTooltipTimer();
    clearTooltipHideTimer();
    clearTooltipFrame();
  }, []);

  useEffect(() => {
    if (open) hideSidebarTooltip();
  }, [open]);

  const handleToggle = () => {
    closeActions();
    hideSidebarTooltip();
    setEditingId(null);
    onToggle();
  };

  const selectConversation = (id: string) => {
    closeActions();
    hideSidebarTooltip();
    onSelect(id);
  };

  const openActions = (
    conversation: SubtitleConversation,
    target: HTMLButtonElement,
    clientX: number,
    clientY: number,
  ) => {
    actionsTriggerRef.current = target;
    setActionsPopoverPosition(actionsPosition(clientX, clientY));
    setChoosingIcon(false);
    setActionsId(conversation.id);
  };

  const startRename = (conversation: SubtitleConversation) => {
    setEditDraft(conversation.title);
    setEditingId(conversation.id);
    closeActions();
  };

  if (!open) {
    return (
      <>
        <aside className="conversation-sidebar conversation-sidebar-collapsed" aria-label={t("conversations.sidebar")}>
          <button
            className="sidebar-icon-button"
            type="button"
            aria-label={t("conversations.expandSidebar")}
            aria-expanded="false"
            onPointerEnter={(event) => showSidebarTooltip(event.currentTarget, t("conversations.expandSidebar"))}
            onPointerLeave={hideSidebarTooltip}
            onFocus={(event) => showSidebarTooltip(event.currentTarget, t("conversations.expandSidebar"), undefined, false, event.currentTarget.matches(":focus-visible"))}
            onBlur={hideSidebarTooltip}
            onClick={handleToggle}
          ><PanelLeftOpen size={19} /></button>
          <button
            className="sidebar-icon-button sidebar-new-icon"
            type="button"
            aria-label={t("conversations.create")}
            onPointerEnter={(event) => showSidebarTooltip(event.currentTarget, t("conversations.create"))}
            onPointerLeave={hideSidebarTooltip}
            onFocus={(event) => showSidebarTooltip(event.currentTarget, t("conversations.create"), undefined, false, event.currentTarget.matches(":focus-visible"))}
            onBlur={hideSidebarTooltip}
            onClick={() => {
              hideSidebarTooltip();
              onNew();
            }}
          ><Plus size={20} /></button>
          <div className="sidebar-rail-divider" />
          <div className="sidebar-conversation-rail" aria-label={t("conversations.recent")} onScroll={hideSidebarTooltip}>
            {conversations.map((conversation) => (
              <RailConversationButton
                key={conversation.id}
                conversation={conversation}
                active={conversation.id === activeId}
                selected={conversation.id === selectedId}
                locale={locale}
                onSelect={selectConversation}
                onShowTooltip={showSidebarTooltip}
                onHideTooltip={hideSidebarTooltip}
              />
            ))}
          </div>
        </aside>
        {sidebarTooltip && createPortal(
          <div
            className={`sidebar-rail-tooltip ${sidebarTooltip.visible ? "visible" : ""} ${sidebarTooltip.keyboard ? "keyboard" : ""}`}
            role="tooltip"
            style={{
              top: sidebarTooltip.top,
              left: sidebarTooltip.left,
              maxWidth: sidebarTooltip.maxWidth,
            }}
          >
            <span className="sidebar-rail-tooltip-title">
              {sidebarTooltip.current && <i aria-hidden="true" />}
              <strong>{sidebarTooltip.title}</strong>
            </span>
            {sidebarTooltip.detail && <span className="sidebar-rail-tooltip-detail">{sidebarTooltip.detail}</span>}
          </div>,
          document.body,
        )}
      </>
    );
  }

  const menuConversation = conversations.find((conversation) => conversation.id === actionsId);

  return (
    <aside className="conversation-sidebar" aria-label={t("conversations.sidebar")}>
      <div className="conversation-sidebar-header">
        <span>{t("conversations.title")}</span>
        <button className="sidebar-icon-button" type="button" aria-label={t("conversations.collapseSidebar")} aria-expanded="true" onClick={handleToggle}><PanelLeftClose size={19} /></button>
      </div>
      <button className="new-conversation-button" type="button" onClick={onNew}><Plus size={18} />{t("conversations.create")}</button>
      <div className="conversation-sidebar-list" onScroll={() => closeActions()}>
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
      </div>
      {menuConversation && actionsPopoverPosition && createPortal(
        <div
          ref={actionsLayerRef}
          className={`conversation-actions-popover ${actionsPopoverPosition.side === "above" ? "above" : ""} ${choosingIcon ? "choosing-icon" : ""}`}
          role={choosingIcon ? "group" : "menu"}
          aria-label={choosingIcon ? t("conversations.chooseIcon") : t("conversations.manage", { title: menuConversation.title })}
          style={{
            top: actionsPopoverPosition.top,
            left: actionsPopoverPosition.left,
          } as CSSProperties}
          onKeyDown={movePopoverFocus}
        >
          {choosingIcon ? (
            <>
              <span className="conversation-icon-picker-label">{t("conversations.chooseIcon")}</span>
              <div className="conversation-icon-picker">
                <button
                  className={menuConversation.icon === "message" ? "selected" : ""}
                  type="button"
                  aria-label={t("conversations.icons.default")}
                  title={t("conversations.icons.default")}
                  onClick={() => {
                    onIconChange(menuConversation.id, null);
                    closeActions();
                  }}
                >
                  <MessageSquareText size={18} />
                </button>
                {CONVERSATION_ICON_KEYS.filter((icon) => icon !== "message").map((icon) => (
                  <button
                    key={icon}
                    className={menuConversation.icon === icon ? "selected" : ""}
                    type="button"
                    aria-label={t(`conversations.icons.${icon}`)}
                    title={t(`conversations.icons.${icon}`)}
                    onClick={() => {
                      onIconChange(menuConversation.id, icon);
                      closeActions();
                    }}
                  >
                    <ConversationIconView icon={icon} size={18} />
                  </button>
                ))}
              </div>
            </>
          ) : (
            <>
              <button type="button" role="menuitem" onClick={() => startRename(menuConversation)}><Pencil size={16} />{t("conversations.rename")}</button>
              <button type="button" role="menuitem" onClick={() => setChoosingIcon(true)}><Shapes size={16} />{t("conversations.changeIcon")}</button>
              {menuConversation.customized && (
                <button type="button" role="menuitem" onClick={() => {
                  onResetCustomization(menuConversation.id);
                  closeActions();
                }}><RotateCcw size={16} />{t("conversations.resetCustomization")}</button>
              )}
            </>
          )}
        </div>,
        document.body,
      )}
    </aside>
  );
}

function ConversationRow({
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
  conversation: SubtitleConversation;
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
    conversation: SubtitleConversation,
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
          <span className="conversation-button-title"><ConversationIconView icon={conversation.icon} size={16} /><strong>{conversation.title}</strong>{active && <i aria-label={t("conversations.current")} />}</span>
          <span className="conversation-button-meta">
            <time>{conversationTime(conversation.startedAt, locale, t("date.today"), t("date.yesterday"))}</time>
            <span>{t("conversations.subtitleCount", { count: conversation.subtitles.length })}</span>
          </span>
        </button>
      )}
    </div>
  );
}

function RailConversationButton({
  conversation,
  active,
  selected,
  locale,
  onSelect,
  onShowTooltip,
  onHideTooltip,
}: {
  conversation: SubtitleConversation;
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

function ConversationIconView({ icon, size }: { icon: ConversationIcon; size: number }) {
  const Icon = ICONS[icon];
  return <Icon size={size} />;
}
